//! Timing inspection uses the public API without attaching or starting a model turn.

use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_fake_paginated_rollout;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::rollout_path;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadTimingEvent;
use codex_app_server_protocol::ThreadTimingPhase;
use codex_app_server_protocol::ThreadTimingReadResponse;
use codex_app_server_protocol::ThreadTimingStatus;
use codex_app_server_protocol::ThreadTimingUpdatedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(20);

async fn rpc(client: &mut TestAppServer, method: &str, params: Value) -> Result<Value> {
    let id = client.send_raw_request(method, Some(params)).await?;
    timeout(WAIT, client.read_response::<Value>(id)).await?
}

#[tokio::test]
async fn timing_reads_are_passive_and_subscriptions_resume_after_the_last_cursor() -> Result<()> {
    let model = create_mock_responses_server_repeating_assistant("Done").await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&model.uri()).write(home.path())?;
    let ts = "2025-01-03T17-30-00";
    let thread = create_fake_paginated_rollout(
        home.path(),
        ts,
        "2025-01-03T17:30:00Z",
        "preview",
        Some("mock_provider"),
        None,
    )?;
    let path = rollout_path(home.path(), ts, &thread);
    let mut client = TestAppServer::builder()
        .with_codex_home(home.path())
        .build()
        .await?;
    client.initialize().await?;
    let loaded_before = rpc(&mut client, "thread/loaded/list", json!({})).await?;
    let before: ThreadTimingReadResponse = serde_json::from_value(
        rpc(
            &mut client,
            "thread/timing/read",
            json!({"threadId":thread,"turnId":"turn"}),
        )
        .await?,
    )?;
    assert!(before.data.is_empty());
    let subscribe = client
        .send_raw_request(
            "thread/timing/subscribe",
            Some(json!({
                "subscriptionId":"watch", "threadId":thread, "turnId":"turn", "cursor":before.cursor
            })),
        )
        .await?;
    let _: Value = timeout(WAIT, client.read_response(subscribe)).await??;
    let ordinal = std::fs::read_to_string(&path)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|line| line["ordinal"].as_u64())
        .max()
        .unwrap()
        + 1;
    writeln!(
        OpenOptions::new().append(true).open(&path)?,
        "{}",
        json!({
            "timestamp":"2025-01-03T17:30:00Z", "ordinal":ordinal, "type":"timing_record", "payload":{
                "version":1,"thread_id":thread,"turn_id":"turn","request_id":"request","attempt":1,
                "span_id":"span","parent_span_id":null,"phase":"model_request","event":"started","at_ms":1234,"model":"test"
            }
        })
    )?;
    let update: ThreadTimingUpdatedNotification =
        timeout(WAIT, client.read_notification("thread/timing/updated")).await??;
    assert_eq!(update.subscription_id, "watch");
    assert_eq!(update.page.data.len(), 1);
    assert_eq!(update.page.data[0].record.span_id, "span");
    rpc(
        &mut client,
        "thread/timing/unsubscribe",
        json!({"subscriptionId":"watch"}),
    )
    .await?;
    let replay: ThreadTimingReadResponse = serde_json::from_value(
        rpc(
            &mut client,
            "thread/timing/read",
            json!({
                "threadId":thread,"turnId":"turn","cursor":before.cursor
            }),
        )
        .await?,
    )?;
    assert_eq!(replay.data, update.page.data);
    let eof: ThreadTimingReadResponse = serde_json::from_value(
        rpc(
            &mut client,
            "thread/timing/read",
            json!({
                "threadId":thread,"turnId":"turn","cursor":replay.cursor
            }),
        )
        .await?,
    )?;
    assert!(eof.data.is_empty());
    assert_eq!(
        rpc(&mut client, "thread/loaded/list", json!({})).await?,
        loaded_before
    );
    assert!(
        model
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() != "POST")
    );
    Ok(())
}

#[tokio::test]
async fn live_timing_reports_pending_receive_wait_then_a_durable_completion() -> Result<()> {
    let (release, gate) = tokio::sync::oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_response_created("response")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![
                responses::ev_assistant_message("message", "done"),
                responses::ev_completed("response"),
            ]),
        },
    ]])
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(model.uri()).write(home.path())?;
    let mut client = TestAppServer::builder()
        .with_codex_home(home.path())
        .build()
        .await?;
    client.initialize().await?;
    let start = client
        .send_thread_start_request_with_auto_env(Default::default())
        .await?;
    let started: ThreadStartResponse = timeout(WAIT, client.read_response(start)).await??;
    let thread = started.thread.id;
    let start = client
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.clone(),
            input: vec![UserInput::Text {
                text: "wait for completion".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn: TurnStartResponse = timeout(WAIT, client.read_response(start)).await??;
    timeout(WAIT, model.wait_for_request_count(1)).await?;
    let subscribe = client
        .send_raw_request(
            "thread/timing/subscribe",
            Some(json!({
                "subscriptionId":"live", "threadId":thread, "turnId":turn.turn.id
            })),
        )
        .await?;
    let _: Value = timeout(WAIT, client.read_response(subscribe)).await??;
    let span_id = timeout(WAIT, async {
        loop {
            let update: ThreadTimingUpdatedNotification =
                client.read_notification("thread/timing/updated").await?;
            if let Some(span) = update.page.active.iter().find(|span| {
                span.record.phase == ThreadTimingPhase::ModelRequest
                    && span
                        .stream
                        .as_ref()
                        .is_some_and(|stream| stream.receive_wait_us > 0)
            }) {
                break anyhow::Ok(span.record.span_id.clone());
            }
        }
    })
    .await??;
    release.send(()).unwrap();
    timeout(WAIT, async {
        loop {
            let update: ThreadTimingUpdatedNotification =
                client.read_notification("thread/timing/updated").await?;
            if update.page.data.iter().any(|entry| {
                entry.record.span_id == span_id
                    && matches!(
                        entry.record.event,
                        ThreadTimingEvent::Completed {
                            status: ThreadTimingStatus::Completed,
                            ..
                        }
                    )
            }) {
                break anyhow::Ok(());
            }
        }
    })
    .await??;
    rpc(
        &mut client,
        "thread/timing/unsubscribe",
        json!({"subscriptionId":"live"}),
    )
    .await?;
    assert_eq!(model.requests().await.len(), 1);
    Ok(())
}
