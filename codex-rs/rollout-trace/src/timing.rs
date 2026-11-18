//! Compact, content-free observations for the canonical session rollout.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_history::timing::StreamTiming;
use codex_history::timing::TimingEvent;
use codex_history::timing::TimingPhase;
use codex_history::timing::TimingRecord;
use codex_history::timing::TimingStatus;
use codex_protocol::ThreadId;
use uuid::Uuid;

/// Request-local metadata queue. Hosts drain it at lifecycle boundaries, never per token.
#[derive(Clone, Debug)]
pub struct TimingContext {
    shared: Arc<TimingShared>,
    attempt: u32,
    parent_span_id: Option<String>,
}

#[derive(Debug)]
struct TimingShared {
    thread_id: ThreadId,
    turn_id: String,
    request_id: String,
    model: String,
    buffer: Mutex<TimingBuffer>,
}

#[derive(Debug, Default)]
struct TimingBuffer {
    pending: Vec<TimingRecord>,
    open_transports: Vec<OpenTransport>,
    active: std::collections::HashMap<String, live::LiveSpan>,
}

#[derive(Debug)]
struct OpenTransport {
    span_id: String,
    parent_span_id: Option<String>,
    attempt: u32,
    started: Instant,
    finished: Arc<AtomicBool>,
}

/// An observed client span. Dropping an unfinished span records cancellation, not success.
#[derive(Debug)]
pub struct TimingSpan {
    context: TimingContext,
    span_id: String,
    phase: TimingPhase,
    started: Instant,
    finished: Arc<AtomicBool>,
}

impl TimingContext {
    pub fn new(thread_id: ThreadId, turn_id: String, model: String) -> Self {
        let context = Self {
            shared: Arc::new(TimingShared {
                thread_id,
                turn_id,
                request_id: Uuid::new_v4().to_string(),
                model,
                buffer: Mutex::new(TimingBuffer::default()),
            }),
            attempt: 0,
            parent_span_id: None,
        };
        live::register(&context.shared);
        context
    }

    pub fn for_attempt(&self, attempt: u32) -> Self {
        Self {
            attempt,
            ..self.clone()
        }
    }

    pub fn under(&self, span: &TimingSpan) -> Self {
        Self {
            parent_span_id: Some(span.span_id.clone()),
            ..self.clone()
        }
    }

    pub fn span(&self, phase: TimingPhase) -> TimingSpan {
        let span_id = Uuid::new_v4().to_string();
        let started = Instant::now();
        let finished = Arc::new(AtomicBool::new(false));
        let mut buffer = self
            .shared
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if phase == TimingPhase::TransportAttempt {
            buffer.open_transports.push(OpenTransport {
                span_id: span_id.clone(),
                parent_span_id: self.parent_span_id.clone(),
                attempt: self.attempt,
                started,
                finished: Arc::clone(&finished),
            });
        }
        let record = self.record(
            &span_id,
            phase,
            TimingEvent::Started {
                at_ms: now_ms(),
                model: self.shared.model.clone(),
            },
        );
        buffer.active.insert(
            span_id.clone(),
            live::LiveSpan {
                record: record.clone(),
                started,
                stream: None,
            },
        );
        buffer.pending.push(record);
        TimingSpan {
            context: self.clone(),
            span_id,
            phase,
            started,
            finished,
        }
    }

    /// Close producer-side attempts when their consumer finishes before a terminal event.
    pub fn cancel_open_transports(&self) {
        let mut buffer = self
            .shared
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let open = std::mem::take(&mut buffer.open_transports);
        for transport in open {
            if !transport.finished.swap(true, Ordering::AcqRel) {
                buffer.active.remove(&transport.span_id);
                let context = Self {
                    attempt: transport.attempt,
                    parent_span_id: transport.parent_span_id,
                    ..self.clone()
                };
                buffer.pending.push(context.record(
                    &transport.span_id,
                    TimingPhase::TransportAttempt,
                    TimingEvent::Completed {
                        at_ms: now_ms(),
                        duration_us: micros(transport.started.elapsed()),
                        status: TimingStatus::Cancelled,
                        response_id: None,
                        stream: None,
                    },
                ));
            }
        }
    }

    pub fn drain(&self) -> Vec<TimingRecord> {
        std::mem::take(
            &mut self
                .shared
                .buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pending,
        )
    }

    fn push(&self, span_id: &str, phase: TimingPhase, event: TimingEvent) {
        self.shared
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .push(self.record(span_id, phase, event));
    }

    fn record(&self, span_id: &str, phase: TimingPhase, event: TimingEvent) -> TimingRecord {
        TimingRecord {
            version: 1,
            thread_id: self.shared.thread_id,
            turn_id: self.shared.turn_id.clone(),
            request_id: self.shared.request_id.clone(),
            attempt: self.attempt,
            span_id: span_id.to_string(),
            parent_span_id: self.parent_span_id.clone(),
            phase,
            event,
        }
    }
}

impl TimingSpan {
    pub fn finish(
        &self,
        status: TimingStatus,
        response_id: Option<String>,
        stream: Option<StreamTiming>,
    ) {
        // Publish the terminal record and retire the transport under the same lock.
        // A consumer draining after cancellation must not outrun producer cleanup.
        let mut buffer = self
            .context
            .shared
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.finished.swap(true, Ordering::AcqRel) {
            buffer.active.remove(&self.span_id);
            if self.phase == TimingPhase::TransportAttempt {
                buffer
                    .open_transports
                    .retain(|transport| transport.span_id != self.span_id);
            }
            buffer.pending.push(self.context.record(
                &self.span_id,
                self.phase,
                TimingEvent::Completed {
                    at_ms: now_ms(),
                    duration_us: micros(self.started.elapsed()),
                    status,
                    response_id,
                    stream,
                },
            ));
        }
    }
}

impl Drop for TimingSpan {
    fn drop(&mut self) {
        self.finish(
            TimingStatus::Cancelled,
            /*response_id*/ None,
            /*stream*/ None,
        );
    }
}

mod live;
mod stream;
pub use live::ActiveTiming;
pub use live::active_timing;
pub use stream::StreamObservation;
pub use stream::StreamTimingCapture;

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "timing_tests.rs"]
mod tests;
