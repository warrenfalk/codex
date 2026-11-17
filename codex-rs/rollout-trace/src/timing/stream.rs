//! Bounded, content-free observations of harness stream events.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use codex_history::timing::ItemBoundary;
use codex_history::timing::ItemTiming;
use codex_history::timing::StreamTiming;
use codex_history::timing::TimingEvent;
use codex_history::timing::TimingPhase;
use codex_history::timing::TimingStatus;
use uuid::Uuid;

use super::TimingContext;
use super::TimingSpan;
use super::micros;
use super::now_ms;

pub(super) const MAX_OPEN_ITEMS: usize = 128;

/// Metadata retained from a received event; it deliberately has no text field.
pub enum StreamObservation<'a> {
    Event,
    UnidentifiedItem,
    Created(&'a str),
    Completed(&'a str),
    Added { id: &'a str, kind: &'a str },
    Done { id: &'a str, kind: &'a str },
    Delta { id: Option<&'a str>, bytes: usize },
}

struct OutputTiming {
    span_id: String,
    metrics: ItemTiming,
}

/// Bounded stream/item counters. Repeated deltas update counters without adding log records.
pub struct StreamTimingCapture {
    context: TimingContext,
    summary: Arc<Mutex<StreamTiming>>,
    open_items: HashMap<String, OutputTiming>,
    active_item: Option<String>,
    response_id: Option<String>,
}

impl StreamTimingCapture {
    pub fn new(context: TimingContext) -> Self {
        Self {
            context,
            summary: Arc::new(Mutex::new(StreamTiming::default())),
            open_items: HashMap::new(),
            active_item: None,
            response_id: None,
        }
    }

    /// Returns whether new durable boundary records are available to flush.
    pub fn observe(&mut self, event: StreamObservation<'_>) -> bool {
        let at_ms = now_ms();
        {
            let mut summary = self
                .summary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            summary.event_count = summary.event_count.saturating_add(1);
            summary.first_event_at_ms.get_or_insert(at_ms);
            summary.last_event_at_ms = Some(at_ms);
        }
        match event {
            StreamObservation::Event => false,
            StreamObservation::UnidentifiedItem => {
                self.active_item = None;
                let mut summary = self
                    .summary
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                summary.omitted_item_count = summary.omitted_item_count.saturating_add(1);
                false
            }
            StreamObservation::Completed(id) => {
                self.response_id = Some(id.to_string());
                false
            }
            StreamObservation::Created(id) => {
                self.response_id = Some(id.to_string());
                self.context.push(
                    &Uuid::new_v4().to_string(),
                    TimingPhase::ModelRequest,
                    TimingEvent::ResponseCreated {
                        at_ms,
                        response_id: id.to_string(),
                    },
                );
                true
            }
            StreamObservation::Added { id, kind } => {
                if self.open_items.contains_key(id) {
                    return false;
                }
                if self.open_items.len() >= MAX_OPEN_ITEMS {
                    self.active_item = None;
                    let mut summary = self
                        .summary
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    summary.omitted_item_count = summary.omitted_item_count.saturating_add(1);
                    return false;
                }
                let span_id = Uuid::new_v4().to_string();
                self.context.push(
                    &span_id,
                    TimingPhase::ModelOutput,
                    TimingEvent::Item {
                        at_ms,
                        item_id: id.to_string(),
                        item_type: kind.to_string(),
                        boundary: ItemBoundary::Added,
                        stream: None,
                    },
                );
                self.open_items.insert(
                    id.to_string(),
                    OutputTiming {
                        span_id,
                        metrics: ItemTiming::default(),
                    },
                );
                self.active_item = (self.open_items.len() == 1).then(|| id.to_string());
                true
            }
            StreamObservation::Done { id, kind } => {
                let output = self.open_items.remove(id);
                let span_id = output
                    .as_ref()
                    .map(|output| output.span_id.clone())
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                self.context.push(
                    &span_id,
                    TimingPhase::ModelOutput,
                    TimingEvent::Item {
                        at_ms,
                        item_id: id.to_string(),
                        item_type: kind.to_string(),
                        boundary: ItemBoundary::Done,
                        stream: output.map(|output| output.metrics),
                    },
                );
                if self.active_item.as_deref() == Some(id) {
                    self.active_item = None;
                }
                true
            }
            StreamObservation::Delta { id, bytes } => {
                let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
                {
                    let mut summary = self
                        .summary
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    summary.delta_count = summary.delta_count.saturating_add(1);
                    summary.delta_bytes = summary.delta_bytes.saturating_add(bytes);
                    summary.first_delta_at_ms.get_or_insert(at_ms);
                    summary.last_delta_at_ms = Some(at_ms);
                }
                // Some adapters cannot identify the source of a text delta. Keep it
                // request-wide when overlapping items make attribution ambiguous.
                let id = id.or(self.active_item.as_deref());
                if let Some(output) = id.and_then(|id| self.open_items.get_mut(id)) {
                    output.metrics.delta_count = output.metrics.delta_count.saturating_add(1);
                    output.metrics.delta_bytes = output.metrics.delta_bytes.saturating_add(bytes);
                    output.metrics.first_delta_at_ms.get_or_insert(at_ms);
                    output.metrics.last_delta_at_ms = Some(at_ms);
                }
                false
            }
        }
    }

    pub fn record_wait(&self, duration: Duration) {
        let mut metrics = self
            .summary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        metrics.receive_wait_us = metrics.receive_wait_us.saturating_add(micros(duration));
    }

    pub fn processing(&self) -> EventProcessingTiming {
        EventProcessingTiming {
            summary: Arc::clone(&self.summary),
            started: Instant::now(),
        }
    }

    pub fn finish(&self, span: &TimingSpan, status: TimingStatus) {
        let summary = self
            .summary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        span.finish(status, self.response_id.clone(), Some(summary));
    }
}

/// Accounts for event processing even when its handler breaks, returns, or is cancelled.
pub struct EventProcessingTiming {
    summary: Arc<Mutex<StreamTiming>>,
    started: Instant,
}

impl Drop for EventProcessingTiming {
    fn drop(&mut self) {
        let mut summary = self
            .summary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        summary.event_processing_us = summary
            .event_processing_us
            .saturating_add(micros(self.started.elapsed()));
    }
}
