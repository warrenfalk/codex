use super::*;
use axum::extract::State;
use axum::routing::post;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::timeout;
use wiremock::MockServer;

#[derive(Clone, Default)]
struct Script {
    requests: Arc<Mutex<Vec<Value>>>,
    fail_first: bool,
}

async fn scripted_chat(State(script): State<Script>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let index = if let Ok(mut requests) = script.requests.lock() {
        let index = requests.len();
        requests.push(request);
        index
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if script.fail_first && index == 0 {
        return (
            [(CONTENT_TYPE, "text/event-stream")],
            "data: {\"error\":{\"message\":\"temporary Chat failure\"}}\n\n",
        )
            .into_response();
    }
    let content = if script.fail_first {
        "recovered"
    } else {
        match index {
            0 => "before compaction",
            1 => "condensed history",
            _ => "after compaction",
        }
    };
    let chunk = json!({
        "id": format!("chatcmpl_{index}"),
        "choices": [{
            "index": 0,
            "delta": {"content": content},
            "finish_reason": "stop"
        }]
    });
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

fn provider(base_url: String, stream_retries: u64) -> ModelProviderInfo {
    let mut provider =
        built_in_model_providers(/* openai_base_url */ /*openai_base_url*/ None)["openai"].clone();
    provider.name = "scripted-resilience-chat-proxy".to_string();
    provider.base_url = Some(base_url);
    provider.env_key = None;
    provider.experimental_bearer_token = None;
    provider.requires_openai_auth = false;
    provider.supports_websockets = false;
    provider.request_max_retries = Some(0);
    provider.stream_max_retries = Some(stream_retries);
    provider
}

async fn build_codex(script: Script, stream_retries: u64) -> anyhow::Result<TestCodex> {
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(scripted_chat))
            .with_state(script),
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
    let provider = provider(format!("http://{proxy}/v1"), stream_retries);
    let current_exe = std::env::current_exe()?;
    let mut builder = test_codex()
        .with_config(move |config| {
            config.base_instructions = Some("Answer concisely.".to_string());
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
    builder.build_with_auto_env(&unused_responses_server).await
}

async fn submit_text(codex: &codex_core::CodexThread, text: &str) -> anyhow::Result<()> {
    codex
        .start_turn_if_idle(TurnInputRequest::user_input(vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_turn_complete(codex).await
}

async fn wait_for_turn_complete(codex: &codex_core::CodexThread) -> anyhow::Result<()> {
    loop {
        let event = timeout(Duration::from_secs(20), codex.next_event()).await??;
        match event.msg {
            EventMsg::Error(error) => anyhow::bail!("Codex turn failed: {}", error.message),
            EventMsg::TurnComplete(_) => return Ok(()),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_provider_compacts_locally_and_continues() -> anyhow::Result<()> {
    let script = Script::default();
    let test_codex = build_codex(script.clone(), /*stream_retries*/ 0).await?;
    submit_text(&test_codex.codex, "before local compaction").await?;
    test_codex.codex.submit(Op::Compact).await?;
    wait_for_turn_complete(&test_codex.codex).await?;
    submit_text(&test_codex.codex, "after local compaction").await?;

    let requests = script
        .requests
        .lock()
        .map_err(|error| anyhow::anyhow!("compaction request lock poisoned: {error}"))?;
    assert_eq!(requests.len(), 3);
    let compact_request = requests[1].to_string();
    assert!(compact_request.contains("before local compaction"));
    let follow_up = requests[2].to_string();
    assert!(follow_up.contains("condensed history"));
    assert!(follow_up.contains("after local compaction"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn codex_retries_a_failed_translated_stream_without_history_corruption() -> anyhow::Result<()>
{
    let script = Script {
        fail_first: true,
        ..Script::default()
    };
    let test_codex = build_codex(script.clone(), /*stream_retries*/ 1).await?;
    submit_text(&test_codex.codex, "recover this turn").await?;

    let requests = script
        .requests
        .lock()
        .map_err(|error| anyhow::anyhow!("retry request lock poisoned: {error}"))?;
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.to_string().contains("recover this turn"))
    );
    Ok(())
}
