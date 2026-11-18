//! Weak, process-local access to active observations. Recording never requires a viewer.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::Weak;
use std::time::Instant;

use codex_history::timing::StreamTiming;
use codex_history::timing::TimingRecord;
use codex_protocol::ThreadId;

use super::TimingShared;
use super::micros;
use super::stream::StreamState;

type Registry = Mutex<HashMap<String, Weak<TimingShared>>>;
static REGISTRY: OnceLock<Registry> = OnceLock::new();

#[derive(Debug)]
pub(super) struct LiveSpan {
    pub record: TimingRecord,
    pub started: Instant,
    pub stream: Option<Arc<Mutex<StreamState>>>,
}

/// Provisional monotonic measurements for an open client span, never backend compute time.
pub struct ActiveTiming {
    pub record: TimingRecord,
    pub elapsed_us: u64,
    pub stream: Option<StreamTiming>,
}

pub(super) fn register(shared: &Arc<TimingShared>) {
    REGISTRY
        .get_or_init(Registry::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(shared.request_id.clone(), Arc::downgrade(shared));
}

impl Drop for TimingShared {
    fn drop(&mut self) {
        if let Some(registry) = REGISTRY.get() {
            registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.request_id);
        }
    }
}

/// Snapshot active requests without retaining their contexts or publishing per-token records.
pub fn active_timing(thread_id: ThreadId, turn_id: Option<&str>) -> Vec<ActiveTiming> {
    let Some(registry) = REGISTRY.get() else {
        return Vec::new();
    };
    let requests: Vec<_> = registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .filter_map(Weak::upgrade)
        .collect();
    let mut result = Vec::new();
    for request in requests {
        if request.thread_id != thread_id || turn_id.is_some_and(|turn| turn != request.turn_id) {
            continue;
        }
        let buffer = request
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for span in buffer.active.values() {
            let stream = span.stream.as_ref().map(|stream| {
                let state = stream
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut metrics = state.metrics.clone();
                if let Some(started) = state.receiving {
                    metrics.receive_wait_us = metrics
                        .receive_wait_us
                        .saturating_add(micros(started.elapsed()));
                }
                if let Some(started) = state.processing {
                    metrics.event_processing_us = metrics
                        .event_processing_us
                        .saturating_add(micros(started.elapsed()));
                }
                metrics
            });
            result.push(ActiveTiming {
                record: span.record.clone(),
                elapsed_us: micros(span.started.elapsed()),
                stream,
            });
        }
    }
    result.sort_by(|left, right| left.record.span_id.cmp(&right.record.span_id));
    result
}
