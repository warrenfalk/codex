use anyhow::Context;
use anyhow::Result;
use app_test_support::create_final_assistant_message_sse_response;
use app_test_support::create_mock_responses_server_sequence;
use app_test_support::create_mock_responses_server_sequence_unchecked;
use app_test_support::create_request_permissions_sse_response;
use app_test_support::to_response;
use codex_app_server_protocol::AdditionalFileSystemPermissions;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::GrantedPermissionProfile;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::PermissionGrantScope;
use codex_app_server_protocol::PermissionsRequestApprovalResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ServerRequestObservedNotification;
use codex_app_server_protocol::ThreadStartedNotification;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput as V2UserInput;
use futures::SinkExt;
use tempfile::TempDir;
use tokio::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;

use super::connection_handling_websocket::WsClient;
use super::connection_handling_websocket::connect_websocket;
use super::connection_handling_websocket::create_config_toml;
use super::connection_handling_websocket::read_jsonrpc_message;
use super::connection_handling_websocket::read_notification_for_method;
use super::connection_handling_websocket::read_response_for_id;
use super::connection_handling_websocket::send_initialize_request;
use super::connection_handling_websocket::send_request;
use super::connection_handling_websocket::spawn_websocket_server;
use super::connection_handling_websocket::start_thread;

#[tokio::test]
async fn firehose_observes_thread_started_without_thread_subscription() -> Result<()> {
    let server = create_mock_responses_server_sequence(Vec::new()).await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri(), "never")?;

    let (mut process, bind_addr) = spawn_websocket_server(codex_home.path()).await?;
    let mut owner = connect_websocket(bind_addr).await?;
    let mut firehose = connect_websocket(bind_addr).await?;

    send_initialize_request(&mut owner, /*id*/ 1, "firehose_owner").await?;
    read_response_for_id(&mut owner, /*id*/ 1).await?;
    send_initialize_request_with_opt_out(
        &mut firehose,
        /*id*/ 2,
        "firehose_observer",
        vec!["thread/started".to_string()],
    )
    .await?;
    read_response_for_id(&mut firehose, /*id*/ 2).await?;

    send_request(&mut firehose, "event/firehose", /*id*/ 3, None).await?;
    read_response_for_id(&mut firehose, /*id*/ 3).await?;

    let thread_id = start_thread(&mut owner, /*id*/ 4).await?;
    let observed_started = read_notification_for_method(&mut firehose, "thread/started").await?;
    let started: ThreadStartedNotification = serde_json::from_value(
        observed_started
            .params
            .expect("thread/started params should be present"),
    )?;
    assert_eq!(started.thread.id, thread_id);

    send_request(
        &mut firehose,
        "thread/unsubscribe",
        /*id*/ 5,
        Some(serde_json::to_value(ThreadUnsubscribeParams { thread_id })?),
    )
    .await?;
    let unsubscribe = read_response_for_id(&mut firehose, /*id*/ 5).await?;
    let unsubscribe = to_response::<ThreadUnsubscribeResponse>(unsubscribe)?;
    assert_eq!(unsubscribe.status, ThreadUnsubscribeStatus::NotSubscribed);

    process
        .kill()
        .await
        .context("failed to stop websocket app-server process")?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn firehose_observed_server_request_response_does_not_resolve_request() -> Result<()> {
    let codex_home = TempDir::new()?;
    let responses = vec![
        create_request_permissions_sse_response("call1")?,
        create_final_assistant_message_sse_response("done")?,
        create_final_assistant_message_sse_response(r#"{"title":"Firehose permission"}"#)?,
    ];
    let server = create_mock_responses_server_sequence_unchecked(responses).await;
    create_request_permissions_config_toml(codex_home.path(), &server.uri())?;

    let (mut process, bind_addr) = spawn_websocket_server(codex_home.path()).await?;
    let mut owner = connect_websocket(bind_addr).await?;
    let mut firehose = connect_websocket(bind_addr).await?;

    send_initialize_request(&mut owner, /*id*/ 1, "firehose_request_owner").await?;
    read_response_for_id(&mut owner, /*id*/ 1).await?;
    send_initialize_request(&mut firehose, /*id*/ 2, "firehose_request_observer").await?;
    read_response_for_id(&mut firehose, /*id*/ 2).await?;
    send_request(&mut firehose, "event/firehose", /*id*/ 3, None).await?;
    read_response_for_id(&mut firehose, /*id*/ 3).await?;

    let thread_id = start_thread(&mut owner, /*id*/ 4).await?;
    send_request(
        &mut owner,
        "turn/start",
        /*id*/ 5,
        Some(serde_json::to_value(TurnStartParams {
            thread_id: thread_id.clone(),
            input: vec![V2UserInput::Text {
                text: "ask something".to_string(),
                text_elements: Vec::new(),
            }],
            model: Some("mock-model".to_string()),
            ..Default::default()
        })?),
    )
    .await?;
    read_response_for_id(&mut owner, /*id*/ 5).await?;

    let observed = read_observed_server_request(&mut firehose).await?;
    let ServerRequest::PermissionsRequestApproval {
        request_id: observed_request_id,
        ..
    } = *observed.request
    else {
        panic!("expected observed PermissionsRequestApproval request");
    };

    let raw_request = read_server_request(&mut owner, "item/permissions/requestApproval").await?;
    assert_eq!(raw_request.id, observed_request_id);
    send_response(
        &mut firehose,
        observed_request_id.clone(),
        serde_json::json!({
            "answers": {
                "confirm_path": { "answers": ["observer"] }
            }
        }),
    )
    .await?;
    assert_no_notification_for_method(
        &mut owner,
        "serverRequest/resolved",
        Duration::from_millis(250),
    )
    .await?;

    send_response(
        &mut owner,
        raw_request.id.clone(),
        serde_json::to_value(PermissionsRequestApprovalResponse {
            permissions: GrantedPermissionProfile {
                network: None,
                file_system: Some(AdditionalFileSystemPermissions {
                    read: None,
                    write: None,
                    glob_scan_max_depth: None,
                    entries: None,
                }),
            },
            scope: PermissionGrantScope::Turn,
            strict_auto_review: None,
        })?,
    )
    .await?;

    let resolved = read_notification_for_method(&mut owner, "serverRequest/resolved").await?;
    let resolved_params = resolved.params.expect("serverRequest/resolved params");
    assert_eq!(
        resolved_params.get("requestId"),
        Some(&serde_json::json!(raw_request.id))
    );

    process
        .kill()
        .await
        .context("failed to stop websocket app-server process")?;
    Ok(())
}

async fn send_initialize_request_with_opt_out(
    stream: &mut WsClient,
    id: i64,
    client_name: &str,
    opt_out_notification_methods: Vec<String>,
) -> Result<()> {
    let params = InitializeParams {
        client_info: ClientInfo {
            name: client_name.to_string(),
            title: Some("WebSocket Firehose Test Client".to_string()),
            version: "0.1.0".to_string(),
        },
        capabilities: Some(InitializeCapabilities {
            experimental_api: false,
            request_attestation: false,
            mcp_server_openai_form_elicitation: false,
            opt_out_notification_methods: Some(opt_out_notification_methods),
        }),
    };
    send_request(
        stream,
        "initialize",
        id,
        Some(serde_json::to_value(params)?),
    )
    .await
}

async fn read_observed_server_request(
    stream: &mut WsClient,
) -> Result<ServerRequestObservedNotification> {
    loop {
        let notification = read_notification_for_method(stream, "serverRequest/observed").await?;
        let params = notification
            .params
            .context("serverRequest/observed params should be present")?;
        let observed = serde_json::from_value::<ServerRequestObservedNotification>(params)?;
        if matches!(
            *observed.request,
            ServerRequest::PermissionsRequestApproval { .. }
        ) {
            return Ok(observed);
        }
    }
}

async fn read_server_request(stream: &mut WsClient, method: &str) -> Result<JSONRPCRequest> {
    loop {
        let message = read_jsonrpc_message(stream).await?;
        if let JSONRPCMessage::Request(request) = message
            && request.method == method
        {
            return Ok(request);
        }
    }
}

async fn send_response(
    stream: &mut WsClient,
    request_id: RequestId,
    result: serde_json::Value,
) -> Result<()> {
    let message = JSONRPCMessage::Response(JSONRPCResponse {
        id: request_id,
        result,
    });
    let payload = serde_json::to_string(&message)?;
    stream
        .send(WebSocketMessage::Text(payload.into()))
        .await
        .context("failed to send websocket response")
}

fn create_request_permissions_config_toml(
    codex_home: &std::path::Path,
    server_uri: &str,
) -> std::io::Result<()> {
    let config_toml = codex_home.join("config.toml");
    std::fs::write(
        config_toml,
        format!(
            r#"
model = "mock-model"
approval_policy = "untrusted"
sandbox_mode = "read-only"

model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0

[features]
request_permissions_tool = true
"#
        ),
    )
}

async fn assert_no_notification_for_method(
    stream: &mut WsClient,
    method: &str,
    wait_for: Duration,
) -> Result<()> {
    let result = timeout(wait_for, read_notification_for_method(stream, method)).await;
    assert!(
        result.is_err(),
        "unexpected `{method}` notification before request owner answered"
    );
    Ok(())
}
