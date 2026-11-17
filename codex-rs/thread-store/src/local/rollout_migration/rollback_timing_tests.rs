use super::RollbackPlanner;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadRolledBackEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutLine;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn rollback_assigns_late_timing_records_to_their_explicit_turn() {
    let thread_id = ThreadId::new();
    let timing = |turn_id: &str| {
        serde_json::from_value::<RolloutItem>(json!({
            "type": "timing_record",
            "payload": {
                "version": 1, "thread_id": thread_id, "turn_id": turn_id,
                "request_id": "request", "attempt": 1, "span_id": "span", "parent_span_id": null,
                "phase": "model_request", "event": "started", "at_ms": 1, "model": "model",
            },
        }))
        .unwrap()
    };
    let mut items = Vec::new();
    for turn_id in ["keep", "remove"] {
        items.push(RolloutItem::EventMsg(EventMsg::TurnStarted(
            TurnStartedEvent {
                turn_attribution: None,
                turn_id: turn_id.to_string(),
                root_turn_id: None,
                trace_id: None,
                started_at: Some(1),
                model_context_window: None,
                collaboration_mode_kind: Default::default(),
            },
        )));
        items.push(RolloutItem::EventMsg(EventMsg::UserMessage(
            UserMessageEvent {
                message: turn_id.to_string(),
                ..UserMessageEvent::default()
            },
        )));
        items.push(timing(turn_id));
    }
    // A producer for the older turn can publish metadata after a newer turn starts.
    items.push(timing("keep"));
    items.push(RolloutItem::EventMsg(EventMsg::ThreadRolledBack(
        ThreadRolledBackEvent { num_turns: 1 },
    )));
    let lines: Vec<_> = items
        .into_iter()
        .map(|item| RolloutLine {
            timestamp: "2026-11-14T12:00:00Z".to_string(),
            ordinal: None,
            item,
        })
        .collect();
    let mut planner = RollbackPlanner::new();
    for line in &lines {
        planner.observe(line).unwrap();
    }
    let plan = planner.finish();
    let surviving: Vec<_> = lines
        .into_iter()
        .enumerate()
        .filter_map(|(index, line)| plan.apply(index, line).unwrap())
        .filter_map(|line| match line.item {
            RolloutItem::TimingRecord(record) => Some(record.turn_id),
            _ => None,
        })
        .collect();
    assert_eq!(surviving, vec!["keep", "keep"]);
}
