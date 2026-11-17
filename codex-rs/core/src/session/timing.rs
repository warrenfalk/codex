//! Adapts content-free timing observations to canonical rollout persistence.

use codex_history::RolloutItem;
use codex_history::timing::TimingStatus;
use codex_rollout_trace::StreamObservation;
use codex_rollout_trace::StreamTimingCapture;
use codex_rollout_trace::TimingContext;

use crate::client_common::ResponseEvent;
use crate::session::Session;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result as CodexResult;

pub(super) async fn flush(sess: &Session, timing: &TimingContext) {
    let records: Vec<_> = timing
        .drain()
        .into_iter()
        .map(RolloutItem::TimingRecord)
        .collect();
    if !records.is_empty() {
        sess.persist_rollout_items(&records).await;
    }
}

pub(super) fn status<T>(result: &CodexResult<T>) -> TimingStatus {
    match result {
        Ok(_) => TimingStatus::Completed,
        Err(error) if matches!(error.details(), CodexErrorDetails::TurnAborted) => {
            TimingStatus::Cancelled
        }
        Err(_) => TimingStatus::Failed,
    }
}

pub(super) fn observe(capture: &mut StreamTimingCapture, event: &ResponseEvent) -> bool {
    let observation = match event {
        ResponseEvent::Created {
            response_id: Some(id),
        } => StreamObservation::Created(id),
        ResponseEvent::Completed { response_id, .. } => StreamObservation::Completed(response_id),
        ResponseEvent::OutputItemAdded(item) => item.id().filter(|id| !id.is_empty()).map_or(
            StreamObservation::UnidentifiedItem,
            |id| StreamObservation::Added {
                id: id.as_str(),
                kind: item.item_type(),
            },
        ),
        ResponseEvent::OutputItemDone(item) => {
            item.id()
                .filter(|id| !id.is_empty())
                .map_or(StreamObservation::Event, |id| StreamObservation::Done {
                    id: id.as_str(),
                    kind: item.item_type(),
                })
        }
        ResponseEvent::ToolCallInputDelta { item_id, delta, .. } => StreamObservation::Delta {
            id: Some(item_id),
            bytes: delta.len(),
        },
        ResponseEvent::OutputTextDelta(delta)
        | ResponseEvent::ReasoningSummaryDelta { delta, .. }
        | ResponseEvent::ReasoningContentDelta { delta, .. } => StreamObservation::Delta {
            id: None,
            bytes: delta.len(),
        },
        ResponseEvent::Created { response_id: None }
        | ResponseEvent::SafetyBuffering(_)
        | ResponseEvent::ServerModel(_)
        | ResponseEvent::ModelVerifications(_)
        | ResponseEvent::TurnModerationMetadata(_)
        | ResponseEvent::GuardianAssessment(_)
        | ResponseEvent::ServerReasoningIncluded(_)
        | ResponseEvent::ReasoningSummaryDone { .. }
        | ResponseEvent::ReasoningSummaryPartAdded { .. }
        | ResponseEvent::RateLimits(_)
        | ResponseEvent::ModelsEtag(_) => StreamObservation::Event,
    };
    capture.observe(observation)
}
