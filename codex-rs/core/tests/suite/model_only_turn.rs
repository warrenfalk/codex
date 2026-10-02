use anyhow::Result;
use codex_core::StartIfIdleSubmission;
use codex_core::TurnExecutionMode;
use codex_core::TurnInputRequest;
use codex_core::TurnInputSubmission;
use codex_core::TurnStartOptions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;

async fn wait_for_turn_complete(test: &core_test_support::test_codex::TestCodex) {
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_only_turn_keeps_history_but_exposes_no_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let first = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_assistant_message("msg-1", "Earlier assistant context."),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;
    let mut builder = test_codex();
    let test = builder.build_with_auto_env(&server).await?;

    let first_submission = test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Earlier user context.".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    assert!(matches!(
        first_submission,
        TurnInputSubmission::Started { .. }
    ));
    wait_for_turn_complete(&test).await;
    first.single_request();

    let rewrite = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-2"),
            responses::ev_assistant_message("msg-2", r#"{"rewritten_prompt":"Clear prompt."}"#),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;
    let rewrite_submission = test
        .codex
        .start_turn_if_idle(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Rewrite this draft.".to_string(),
                text_elements: Vec::new(),
            }])
            .on_start(TurnStartOptions {
                final_output_json_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "rewritten_prompt": { "type": "string" } },
                    "required": ["rewritten_prompt"],
                    "additionalProperties": false
                })),
                execution_mode: TurnExecutionMode::ModelOnly,
                ..Default::default()
            }),
        )
        .await?;
    assert!(matches!(
        rewrite_submission,
        StartIfIdleSubmission::Started { .. }
    ));
    wait_for_turn_complete(&test).await;

    let body = rewrite.single_request().body_json();
    assert_eq!(body["tools"], serde_json::json!([]));
    let serialized_input = serde_json::to_string(&body["input"])?;
    assert!(serialized_input.contains("Earlier user context."));
    assert!(serialized_input.contains("Earlier assistant context."));
    assert!(serialized_input.contains("Rewrite this draft."));
    Ok(())
}
