//! Request timing survives replay without entering model-visible history.

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_history::RolloutItem;
use codex_history::timing::ItemBoundary;
use codex_history::timing::TimingEvent;
use codex_history::timing::TimingPhase;
use codex_history::timing::TimingRecord;
use codex_history::timing::TimingStatus;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::*;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;

async fn records(path: &std::path::Path) -> Vec<TimingRecord> {
    let (items, _, parse_errors) = codex_rollout::RolloutRecorder::load_rollout_items(path)
        .await
        .expect("timing records remain readable on resume");
    assert_eq!(parse_errors, 0);
    items
        .into_iter()
        .filter_map(|item| match item {
            RolloutItem::TimingRecord(record) => Some(record),
            _ => None,
        })
        .collect()
}

fn completed_models(records: &[TimingRecord]) -> Vec<(&TimingRecord, TimingStatus)> {
    records
        .iter()
        .filter_map(|record| match &record.event {
            TimingEvent::Completed { status, .. } if record.phase == TimingPhase::ModelRequest => {
                Some((record, *status))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reasoning_items_tools_and_resumed_requests_have_correlated_timing() -> Result<()> {
    let server = start_mock_server().await;
    let plan = json!({"plan":[{"step":"observe timing","status":"in_progress"}]}).to_string();
    let response_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("response-a"),
                ev_reasoning_item_added("reasoning-a", &[]),
                ev_reasoning_summary_text_delta("private timing fixture"),
                ev_reasoning_item("reasoning-a", &["private timing fixture"], &[]),
                ev_reasoning_item_added("reasoning-b", &[]),
                ev_reasoning_item("reasoning-b", &[], &[]),
                ev_function_call("plan-call", "update_plan", &plan),
                ev_completed("response-a"),
            ]),
            sse(vec![
                ev_response_created("response-b"),
                ev_assistant_message("message-b", "done"),
                ev_completed("response-b"),
            ]),
            sse(vec![
                ev_response_created("response-c"),
                ev_assistant_message("message-c", "resumed"),
                ev_completed("response-c"),
            ]),
        ],
    )
    .await;
    let test = test_codex().build_with_auto_env(&server).await?;
    let path = test.codex.rollout_path().expect("rollout path");
    test.submit_turn("first turn").await?;
    test.codex.shutdown_and_wait().await?;
    let resumed = test_codex()
        .resume(&server, test.home.clone(), path.clone())
        .await?;
    resumed.submit_turn("resume this session").await?;
    resumed.codex.shutdown_and_wait().await?;

    let timing = records(&path).await;
    let completed = completed_models(&timing);
    assert_eq!(completed.len(), 3);
    assert_eq!(
        completed
            .iter()
            .map(|(_, status)| *status)
            .collect::<Vec<_>>(),
        vec![TimingStatus::Completed; 3]
    );
    assert_ne!(completed[0].0.request_id, completed[1].0.request_id);
    assert_eq!(completed[0].0.turn_id, completed[1].0.turn_id);
    assert_ne!(completed[1].0.turn_id, completed[2].0.turn_id);
    let reasoning: Vec<_> = timing
        .iter()
        .filter_map(|record| match &record.event {
            TimingEvent::Item {
                item_id,
                item_type,
                boundary,
                ..
            } if item_type == "reasoning" => {
                assert_eq!(record.request_id, completed[0].0.request_id);
                Some((item_id.as_str(), *boundary))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        reasoning,
        vec![
            ("reasoning-a", ItemBoundary::Added),
            ("reasoning-a", ItemBoundary::Done),
            ("reasoning-b", ItemBoundary::Added),
            ("reasoning-b", ItemBoundary::Done)
        ]
    );
    assert!(
        timing
            .iter()
            .any(|record| record.phase == TimingPhase::ToolWait)
    );
    assert!(
        timing
            .iter()
            .any(|record| record.phase == TimingPhase::TransportAttempt)
    );
    let serialized = serde_json::to_string(&timing)?;
    assert!(!serialized.contains("private timing fixture"));
    let requests = response_mock.requests();
    assert_eq!(requests.len(), 3);
    for request in requests {
        assert!(!request.body_json().to_string().contains("timing_record"));
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stream_recovery_has_distinct_attempts_under_one_request() -> Result<()> {
    let server = start_mock_server().await;
    let response_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![ev_response_created("interrupted-response")]),
            sse(vec![
                ev_response_created("recovered-response"),
                ev_assistant_message("message", "done"),
                ev_completed("recovered-response"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.stream_max_retries = Some(1);
        })
        .build_with_auto_env(&server)
        .await?;
    let path = test.codex.rollout_path().expect("rollout path");
    test.submit_turn("recover from an early stream close")
        .await?;
    test.codex.shutdown_and_wait().await?;
    assert_eq!(response_mock.requests().len(), 2);
    let timing = records(&path).await;
    let completed = completed_models(&timing);
    assert_eq!(
        completed
            .iter()
            .map(|(record, status)| (record.attempt, *status))
            .collect::<Vec<_>>(),
        vec![(1, TimingStatus::Failed), (2, TimingStatus::Completed)]
    );
    assert_eq!(completed[0].0.request_id, completed[1].0.request_id);
    assert!(
        timing
            .iter()
            .any(|record| record.phase == TimingPhase::RetryRecovery)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_stream_records_cancellation_before_shutdown() -> Result<()> {
    let (release, gate) = tokio::sync::oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: sse(vec![ev_response_created("pending-response")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: sse(vec![ev_completed("pending-response")]),
        },
    ]])
    .await;
    let test = test_codex().build_with_streaming_server(&server).await?;
    let path = test.codex.rollout_path().expect("rollout path");
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "wait for the model".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    server.wait_for_request_count(1).await;
    test.codex.submit(Op::Interrupt).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    test.codex.shutdown_and_wait().await?;
    let timing = records(&path).await;
    let completed = completed_models(&timing);
    assert_eq!(
        completed
            .iter()
            .map(|(_, status)| *status)
            .collect::<Vec<_>>(),
        vec![TimingStatus::Cancelled]
    );
    assert!(
        timing
            .iter()
            .any(|record| record.phase == TimingPhase::TransportAttempt
                && matches!(
                    record.event,
                    TimingEvent::Completed {
                        status: TimingStatus::Cancelled,
                        ..
                    }
                ))
    );
    drop(release);
    server.shutdown().await;
    Ok(())
}
