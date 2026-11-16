use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartModelOnlyParams;
use codex_app_server_protocol::TurnStartModelOnlyResponse;
use codex_app_server_protocol::UserInput;
use codex_protocol::openai_models::ReasoningEffort;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn turn_start_model_only_uses_public_rpc_and_exposes_no_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_assistant_message("msg-1", r#"{"rewritten_prompt":"Clear."}"#),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri())?;

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;
    let thread_request = app
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let thread_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(thread_request)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response::<ThreadStartResponse>(thread_response)?;

    let params = TurnStartModelOnlyParams {
        thread_id: thread.id,
        input: vec![UserInput::Text {
            text: "Rewrite this.".to_string(),
            text_elements: Vec::new(),
        }],
        model: "mock-model".to_string(),
        effort: Some(ReasoningEffort::Medium),
        output_schema: Some(serde_json::json!({
            "type": "object",
            "properties": { "rewritten_prompt": { "type": "string" } },
            "required": ["rewritten_prompt"],
            "additionalProperties": false
        })),
    };
    let turn_request = app
        .send_raw_request("turn/startModelOnly", Some(serde_json::to_value(params)?))
        .await?;
    let turn_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(turn_request)),
    )
    .await??;
    let response = to_response::<TurnStartModelOnlyResponse>(turn_response)?;
    assert!(!response.turn.id.is_empty());
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let request = response_mock.single_request().body_json();
    assert_eq!(request["tools"], serde_json::json!([]));
    assert!(serde_json::to_string(&request["input"])?.contains("Rewrite this."));
    Ok(())
}

fn create_config_toml(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
}
