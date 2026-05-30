use anyhow::Context;
use anyhow::Result;
use app_test_support::DISABLE_AUTO_THREAD_TITLE_FOR_TESTS_ENV_VAR;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadNameUpdatedNotification;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput as V2UserInput;
use codex_features::Feature;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::TempDir;
use tokio::time::timeout;

#[cfg(windows)]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);
#[cfg(not(windows))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn automatic_thread_title_emits_thread_name_updated() -> Result<()> {
    let codex_home = TempDir::new()?;
    let server =
        create_mock_responses_server_repeating_assistant(r#"{"title":"Parser bug fix"}"#).await;
    write_mock_responses_config_toml(
        codex_home.path(),
        &server.uri(),
        &BTreeMap::from([(Feature::Sqlite, true)]),
        i64::MAX,
        None,
        "mock_provider",
        "compact",
    )?;

    let mut mcp = TestAppServer::new_with_env(
        codex_home.path(),
        &[(DISABLE_AUTO_THREAD_TITLE_FOR_TESTS_ENV_VAR, None)],
    )
    .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let thread_start_id = mcp
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let thread_start_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(thread_start_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(thread_start_resp)?;

    let turn_start_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            input: vec![V2UserInput::Text {
                text: "please fix the parser bug in the lexer".to_string(),
                text_elements: Vec::new(),
            }],
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let turn_start_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turn_start_id)),
    )
    .await??;
    let _: TurnStartResponse = to_response(turn_start_resp)?;

    let notification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("thread/name/updated"),
    )
    .await??;
    let notification: ThreadNameUpdatedNotification = serde_json::from_value(
        notification
            .params
            .context("thread/name/updated notification should include params")?,
    )?;
    assert_eq!(notification.thread_id, thread.id);
    assert_eq!(notification.thread_name.as_deref(), Some("Parser bug fix"));

    Ok(())
}
