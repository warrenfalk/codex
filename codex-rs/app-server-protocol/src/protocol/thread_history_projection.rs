//! Stateless projection from canonical paginated rollout records to thread-history changes.
//!
//! This module is only for the new paginated rollout format that persists canonical
//! `ItemCompleted(TurnItem)` records, not legacy event-only rollouts.

use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutLine;

use crate::protocol::thread_history::ThreadHistoryChangeSet;
use crate::protocol::thread_history::ThreadHistoryItemChange;
use crate::protocol::thread_history::ThreadHistoryTurnMetadata;
use crate::protocol::v2::ThreadItem;
use crate::protocol::v2::ThreadItemPayload;
use crate::protocol::v2::TurnError;
use crate::protocol::v2::TurnStatus;

/// Project one durable rollout line without reconstructing earlier history.
///
/// Callers that replay a JSONL suffix should invoke it once per line, in ordinal order, so storage
/// can preserve the first and latest timestamps for repeated item snapshots independently.
pub fn project_rollout_line(line: &RolloutLine) -> ThreadHistoryChangeSet {
    match &line.item {
        RolloutItem::EventMsg(EventMsg::TurnStarted(event)) => ThreadHistoryChangeSet {
            changed_turns: vec![ThreadHistoryTurnMetadata {
                turn_id: event.turn_id.clone(),
                root_turn_id: event.root_turn_id.clone(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: event.started_at,
                completed_at: None,
                duration_ms: None,
            }],
            ..Default::default()
        },
        RolloutItem::EventMsg(EventMsg::TurnComplete(event)) => ThreadHistoryChangeSet {
            changed_turns: vec![ThreadHistoryTurnMetadata {
                turn_id: event.turn_id.clone(),
                root_turn_id: event.root_turn_id.clone(),
                status: if event.error.is_some() {
                    TurnStatus::Failed
                } else {
                    TurnStatus::Completed
                },
                error: event.error.as_ref().map(|error| TurnError {
                    misalignment: error.misalignment.clone().map(Into::into),
                    message: error.message.clone(),
                    codex_error_info: error.codex_error_info.clone().map(Into::into),
                    additional_details: None,
                }),
                started_at: event.started_at,
                completed_at: event.completed_at,
                duration_ms: event.duration_ms,
            }],
            ..Default::default()
        },
        RolloutItem::EventMsg(EventMsg::TurnAborted(event)) => {
            let Some(turn_id) = event.turn_id.as_ref() else {
                return ThreadHistoryChangeSet::default();
            };
            ThreadHistoryChangeSet {
                changed_turns: vec![ThreadHistoryTurnMetadata {
                    turn_id: turn_id.clone(),
                    root_turn_id: event.root_turn_id.clone(),
                    status: TurnStatus::Interrupted,
                    error: event.error.as_ref().map(|error| TurnError {
                        message: error.message.clone(),
                        codex_error_info: error.codex_error_info.clone().map(Into::into),
                        misalignment: error.misalignment.clone().map(Into::into),
                        additional_details: None,
                    }),
                    started_at: event.started_at,
                    completed_at: event.completed_at,
                    duration_ms: event.duration_ms,
                }],
                ..Default::default()
            }
        }
        RolloutItem::EventMsg(EventMsg::NoteToSelf(event)) => {
            let Some(ordinal) = line.ordinal else {
                return ThreadHistoryChangeSet::default();
            };
            let active_turn_id = event.active_turn_id.clone();
            let turn_id = active_turn_id
                .clone()
                .unwrap_or_else(|| format!("rollout-{ordinal}"));
            let changed_turns = if active_turn_id.is_some() {
                Vec::new()
            } else {
                vec![ThreadHistoryTurnMetadata {
                    turn_id: turn_id.clone(),
                    root_turn_id: None,
                    status: TurnStatus::Completed,
                    error: None,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                }]
            };
            ThreadHistoryChangeSet {
                changed_items: vec![ThreadHistoryItemChange {
                    turn_id,
                    item: ThreadItem::untimed(ThreadItemPayload::NoteToSelf {
                        id: event
                            .item_id
                            .clone()
                            .unwrap_or_else(|| format!("note-{ordinal}")),
                        note: event.note.clone(),
                    }),
                }],
                changed_turns,
                ..Default::default()
            }
        }
        RolloutItem::EventMsg(EventMsg::ItemCompleted(event)) => ThreadHistoryChangeSet {
            changed_items: vec![ThreadHistoryItemChange {
                turn_id: event.turn_id.clone(),
                item: ThreadItem::from(event),
            }],
            ..Default::default()
        },
        RolloutItem::ResponseItem(envelope) => {
            let ResponseItem::Message {
                id, role, content, ..
            } = &envelope.item
            else {
                return ThreadHistoryChangeSet::default();
            };
            // Ordinary model messages already have canonical ItemCompleted records.
            if role != "assistant" {
                return ThreadHistoryChangeSet::default();
            }
            let Some(side_thread_id) = id
                .as_deref()
                .and_then(|id| id.strip_prefix("msg_side_summary_"))
                .filter(|id| !id.is_empty())
            else {
                return ThreadHistoryChangeSet::default();
            };
            let Some(text) = super::thread_history::side_conversation_summary_text(content) else {
                return ThreadHistoryChangeSet::default();
            };
            let turn_id = format!("side-summary-{side_thread_id}");
            ThreadHistoryChangeSet {
                changed_turns: vec![ThreadHistoryTurnMetadata {
                    turn_id: turn_id.clone(),
                    root_turn_id: None,
                    status: TurnStatus::Completed,
                    error: None,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                }],
                changed_items: vec![ThreadHistoryItemChange {
                    turn_id: turn_id.clone(),
                    item: ThreadItem::untimed(ThreadItemPayload::AgentMessage {
                        id: turn_id,
                        text,
                        moderation: None,
                        phase: None,
                        memory_citation: None,
                        delivery: None,
                        questions: None,
                    }),
                }],
                ..Default::default()
            }
        }
        RolloutItem::DynamicToolsUpdated { .. }
        | RolloutItem::SessionMeta(_)
        | RolloutItem::InterAgentCommunication(_)
        | RolloutItem::InterAgentCommunicationMetadata { .. }
        | RolloutItem::Compacted(_)
        | RolloutItem::TurnContext(_)
        | RolloutItem::TokenUsageRecord(_)
        | RolloutItem::TimingRecord(_)
        | RolloutItem::WorldState(_)
        | RolloutItem::RealtimeItem(_)
        | RolloutItem::RetainedContext(_)
        | RolloutItem::SecurityRiskScore(_)
        | RolloutItem::EventMsg(_) => ThreadHistoryChangeSet::default(),
    }
}

#[cfg(test)]
#[path = "thread_history_projection_tests.rs"]
mod tests;
