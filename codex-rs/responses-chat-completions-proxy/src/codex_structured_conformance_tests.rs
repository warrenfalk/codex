use super::*;
use axum::extract::State;
use axum::routing::post;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnStartOptions;
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
struct Capture(Arc<Mutex<Option<Value>>>);

async fn structured_chat(State(capture): State<Capture>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    if let Ok(mut slot) = capture.0.lock() {
        *slot = Some(request);
    }
    let chunks = [
        json!({
            "id": "chatcmpl_json",
            "choices": [{
                "index": 0,
                "delta": {"content": "{\"answer\":"},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl_json",
            "choices": [{
                "index": 0,
                "delta": {"content": "\"ok\"}"},
                "finish_reason": "stop"
            }]
        }),
    ];
    let mut response = chunks
        .into_iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect::<String>();
    response.push_str("data: [DONE]\n\n");
    ([(CONTENT_TYPE, "text/event-stream")], response).into_response()
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
async fn unmodified_codex_forwards_an_explicit_output_schema() -> anyhow::Result<()> {
    let capture = Capture::default();
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(structured_chat))
            .with_state(capture.clone()),
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
            structured_output: true,
            ..BackendCapabilities::default()
        },
    })?)
    .await?;
    let mut provider =
        built_in_model_providers(/* openai_base_url */ /*openai_base_url*/ None)["openai"].clone();
    provider.name = "scripted-structured-chat-proxy".to_string();
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
            config.base_instructions = Some("Return the requested JSON.".to_string());
            config.model_provider = provider;
            config.codex_self_exe = Some(current_exe);
            assert!(config.web_search_mode.set(WebSearchMode::Disabled).is_ok());
        })
        .with_model_info_override("gpt-5.5", |model| {
            model.shell_type = ConfigShellToolType::Disabled;
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
    let schema = json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false
    });
    codex
        .start_turn_if_idle(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Return an answer.".to_string(),
                text_elements: Vec::new(),
            }])
            .on_start(TurnStartOptions {
                final_output_json_schema: Some(schema.clone()),
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

    assert_eq!(final_message.as_deref(), Some("{\"answer\":\"ok\"}"));
    let request = capture
        .0
        .lock()
        .map_err(|error| anyhow::anyhow!("structured capture lock poisoned: {error}"))?
        .clone()
        .ok_or_else(|| anyhow::anyhow!("structured Chat request was not captured"))?;
    assert_eq!(request["response_format"]["type"], json!("json_schema"));
    assert_eq!(request["response_format"]["json_schema"]["schema"], schema);
    Ok(())
}
