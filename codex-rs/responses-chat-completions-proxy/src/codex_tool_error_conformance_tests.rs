use super::*;
use axum::extract::State;
use axum::routing::post;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::timeout;
use wiremock::MockServer;

#[derive(Clone, Default)]
struct ToolErrorScript {
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn scripted_chat(State(script): State<ToolErrorScript>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let request_index = if let Ok(mut requests) = script.requests.lock() {
        let request_index = requests.len();
        requests.push(request.clone());
        request_index
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if request_index == 0 {
        let custom_name = request["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool.pointer("/function/name").and_then(Value::as_str))
            .find(|name| name.starts_with("codex_custom_"));
        let Some(custom_name) = custom_name else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Codex offered no translated custom tool",
            )
                .into_response();
        };
        let arguments = json!({"input": "not a patch"}).to_string();
        return chat_sse(json!({
            "id": "chatcmpl_invalid_patch",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_invalid_patch",
                        "type": "function",
                        "function": {"name": custom_name, "arguments": arguments}
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }));
    }

    chat_sse(json!({
        "id": "chatcmpl_after_tool_error",
        "choices": [{
            "index": 0,
            "delta": {"content": "tool error recovered"},
            "finish_reason": "stop"
        }]
    }))
}

fn chat_sse(chunk: Value) -> Response {
    (
        [(CONTENT_TYPE, "text/event-stream")],
        format!("data: {chunk}\n\ndata: [DONE]\n\n"),
    )
        .into_response()
}

async fn spawn_server(app: Router) -> anyhow::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(address)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unmodified_codex_recovers_from_a_model_visible_tool_error() -> anyhow::Result<()> {
    let script = ToolErrorScript::default();
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(scripted_chat))
            .with_state(script.clone()),
    )
    .await?;
    let proxy = spawn_server(router(ProxyConfig {
        listen_address: "127.0.0.1".parse()?,
        port: 0,
        upstream_url: format!("http://{upstream}/v1/chat/completions"),
        upstream_model: None,
        upstream_bearer: None,
        forward_inbound_authorization: false,
        server_info: None,
        request_timeout: Duration::from_secs(5),
        stream_idle_timeout: Duration::from_secs(5),
        capabilities: BackendCapabilities {
            parallel_tool_calls: true,
            ..BackendCapabilities::default()
        },
    })?)
    .await?;
    let mut provider =
        built_in_model_providers(/* openai_base_url */ /*openai_base_url*/ None)["openai"].clone();
    provider.name = "scripted-tool-error-chat-proxy".to_string();
    provider.base_url = Some(format!("http://{proxy}/v1"));
    provider.env_key = None;
    provider.experimental_bearer_token = None;
    provider.requires_openai_auth = false;
    provider.supports_websockets = false;
    provider.request_max_retries = Some(0);
    provider.stream_max_retries = Some(0);
    let current_exe = std::env::current_exe()?;
    let mut builder = test_codex()
        .with_config(move |config| {
            config.base_instructions = Some("Try the requested patch, then answer.".to_string());
            config.model_provider = provider;
            config.codex_self_exe = Some(current_exe);
            assert!(config.web_search_mode.set(WebSearchMode::Disabled).is_ok());
        })
        .with_model_info_override("gpt-5.5", |model| {
            model.shell_type = ConfigShellToolType::Disabled;
            model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
            model.default_reasoning_level = None;
            model.supports_reasoning_summary_parameter = false;
            model.support_verbosity = false;
            model.supports_search_tool = false;
            model.use_responses_lite = false;
        });
    let unused_responses_server = MockServer::start().await;
    let codex = builder
        .build_with_auto_env(&unused_responses_server)
        .await?
        .codex;

    codex
        .start_turn_if_idle(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Try an invalid patch and recover.".to_string(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(SandboxPolicy::DangerFullAccess),
                ..Default::default()
            }),
        )
        .await?;
    let mut final_message = None;
    loop {
        let event = timeout(Duration::from_secs(20), codex.next_event()).await??;
        match event.msg {
            EventMsg::AgentMessage(message) => final_message = Some(message.message),
            EventMsg::Error(error) => anyhow::bail!("Codex turn failed: {}", error.message),
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }

    assert_eq!(final_message.as_deref(), Some("tool error recovered"));
    let requests = script
        .requests
        .lock()
        .map_err(|error| anyhow::anyhow!("tool-error script lock poisoned: {error}"))?;
    assert_eq!(requests.len(), 2);
    let tool_output = requests[1]["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|message| {
            message["role"] == "tool" && message["tool_call_id"] == "call_invalid_patch"
        })
        .and_then(|message| message["content"].as_str());
    assert!(
        tool_output.is_some_and(|content| content.contains("apply_patch")),
        "translated Chat history did not expose the tool failure: {tool_output:?}"
    );
    Ok(())
}
