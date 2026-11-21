use super::*;
use crate::UpstreamProtocol;
use crate::router_with_protocol;
use codex_core::CodexThread;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task::JoinHandle;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn codex_preserves_gemini_request_prefix_across_tool_retry_and_resume() -> anyhow::Result<()>
{
    let upstream = MockServer::start().await;
    let args = json!({"plan": [{"step": "Confirm tool replay", "status": "completed"}]});
    let parts = json!([
        {"text": "Checking."},
        {
            "functionCall": {"name": "update_plan", "id": "gemini-call-1", "args": args},
            "thoughtSignature": "opaque-function-signature",
        },
        {"functionCall": {"name": "update_plan", "id": "gemini-call-2", "args": args}},
    ]);
    for result in [
        json!({"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP"}]}),
        json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "partial response"}]}}]}),
        json!({"candidates": [{"content": {"role": "model", "parts": [
            {"text": "tool complete"},
            {"text": "", "thoughtSignature": "opaque-text-signature"}
        ]}, "finishReason": "STOP"}]}),
        json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "resumed"}]}, "finishReason": "STOP"}]}),
    ] {
        Mock::given(method("POST"))
            .and(path("/v1beta/models/gemini-4-neon:streamGenerateContent"))
            .and(header("x-goog-api-key", "mock-key"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(format!("data: {result}\n\n"), "text/event-stream"),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&upstream)
            .await;
    }

    let (address, proxy) = start_test_proxy(&upstream).await?;
    let unused_responses_server = MockServer::start().await;
    let original = test_codex_for_proxy(address)
        .build_with_auto_env(&unused_responses_server)
        .await?;
    assert!(
        run_turn(&original.codex, "Update the plan and report completion.")
            .await?
            .contains("tool complete")
    );

    proxy.abort();
    let _ = proxy.await;
    let (address, proxy) = start_test_proxy(&upstream).await?;
    let cwd = original.config.cwd.clone();
    let resumed = test_codex_for_proxy(address)
        .with_config(move |config| config.cwd = cwd)
        .restart_with_auto_env(&unused_responses_server, &original)
        .await?;
    assert!(
        run_turn(&resumed.codex, "What did you complete?")
            .await?
            .contains("resumed")
    );

    let requests = upstream
        .received_requests()
        .await
        .expect("request recording");
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[1].body, requests[2].body, "retry request body");
    let initial: Value = requests[0].body_json()?;
    let continued: Value = requests[1].body_json()?;
    let resumed: Value = requests[3].body_json()?;

    assert_request_prefix(&initial, &continued);
    assert_request_prefix(&continued, &resumed);

    let contents = continued["contents"].as_array().expect("native contents");
    assert_eq!(
        &contents[initial["contents"]
            .as_array()
            .expect("initial contents")
            .len()..],
        &[
            json!({"role": "model", "parts": parts}),
            json!({"role": "user", "parts": [
                {"functionResponse": {"id": "gemini-call-1", "name": "update_plan", "response": {"output": "Plan updated"}}},
                {"functionResponse": {"id": "gemini-call-2", "name": "update_plan", "response": {"output": "Plan updated"}}}
            ]}),
        ],
    );
    let resumed_contents = resumed["contents"].as_array().expect("resumed contents");
    assert_eq!(
        resumed_contents.get(contents.len()),
        Some(&json!({"role": "model", "parts": [
            {"text": "tool complete"},
            {"text": "", "thoughtSignature": "opaque-text-signature"}
        ]})),
    );
    assert!(
        resumed_contents
            .iter()
            .skip(contents.len() + 1)
            .any(|content| content["role"] == "user"
                && content["parts"].as_array().is_some_and(|parts| parts
                    .iter()
                    .any(|part| part["text"] == "What did you complete?"))),
        "resume must append the new user turn"
    );
    proxy.abort();
    Ok(())
}

fn assert_request_prefix(previous: &Value, next: &Value) {
    let prefix_len = previous["contents"]
        .as_array()
        .expect("native contents")
        .len();
    let mut prefix = next.clone();
    let contents = prefix["contents"].as_array_mut().expect("native contents");
    assert!(
        contents.len() > prefix_len,
        "continuation must extend history"
    );
    contents.truncate(prefix_len);
    assert_eq!(&prefix, previous, "request fields and conversation prefix");
}

async fn start_test_proxy(upstream: &MockServer) -> anyhow::Result<(SocketAddr, JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = router_with_protocol(
        ProxyConfig {
            listen_address: address.ip(),
            port: address.port(),
            upstream_url: format!(
                "{}/v1beta/models/gemini-4-neon:streamGenerateContent?alt=sse",
                upstream.uri()
            ),
            upstream_model: None,
            upstream_bearer: Some("mock-key".to_string()),
            forward_inbound_authorization: false,
            server_info: None,
            request_timeout: Duration::from_secs(5),
            stream_idle_timeout: Duration::from_secs(5),
            capabilities: BackendCapabilities::default(),
        },
        UpstreamProtocol::Gemini,
    )?;
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve test proxy");
    });
    Ok((address, task))
}

fn test_codex_for_proxy(address: SocketAddr) -> TestCodexBuilder {
    let mut provider = built_in_model_providers(/*openai_base_url*/ None)["openai"].clone();
    provider.name = "scripted-gemini-proxy".to_string();
    provider.base_url = Some(format!("http://{address}/v1"));
    provider.env_key = None;
    provider.experimental_bearer_token = None;
    provider.requires_openai_auth = false;
    provider.supports_websockets = false;
    provider.request_max_retries = Some(0);
    provider.stream_max_retries = Some(1);
    test_codex()
        .with_config(move |config| {
            config.base_instructions = Some("Use the requested tool, then answer.".to_string());
            config.model_provider = provider;
            config.update_plan_enabled = true;
            assert!(config.web_search_mode.set(WebSearchMode::Disabled).is_ok());
        })
        .with_model_info_override("gpt-5.5", |model| {
            model.shell_type = ConfigShellToolType::Disabled;
            model.supports_reasoning_summary_parameter = false;
            model.support_verbosity = false;
            model.supports_search_tool = false;
            model.use_responses_lite = false;
        })
}

async fn run_turn(codex: &CodexThread, prompt: &str) -> anyhow::Result<String> {
    codex
        .start_turn_if_idle(TurnInputRequest::user_input(vec![UserInput::Text {
            text: prompt.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut streamed = String::new();
    loop {
        match timeout(Duration::from_secs(20), codex.next_event())
            .await??
            .msg
        {
            EventMsg::AgentMessageContentDelta(delta) => streamed.push_str(&delta.delta),
            EventMsg::Error(error) => anyhow::bail!("Codex turn failed: {}", error.message),
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }
    Ok(streamed)
}

#[test]
fn signed_empty_text_and_multimodal_tool_result_keep_their_positions() -> anyhow::Result<()> {
    let replay = json!([
        {"kind": "text", "bytes": 2},
        {"kind": "function", "call_id": "image-call", "signature": "sig"},
        {"kind": "text", "bytes": 0, "signature": "text-sig"},
    ]);
    let input = vec![
        json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "é"}]}),
        json!({"type": "reasoning", "summary": [], "encrypted_content": format!("{REPLAY_PREFIX}{replay}")}),
        json!({"type": "function_call", "call_id": "image-call", "name": "view_image", "arguments": "{}"}),
        json!({"type": "function_call_output", "call_id": "image-call", "output": [
            {"type": "input_text", "text": "See this"},
            {"type": "input_image", "image_url": "data:image/png;base64,YQ=="},
            {"type": "input_text", "text": "End of result"}
        ]}),
    ];
    let history = history::compile(String::new(), input, &ToolRegistry::default())?;
    assert_eq!(
        history.contents,
        vec![
            json!({"role": "model", "parts": [
                {"text": "é"},
                {"functionCall": {"id": "image-call", "name": "view_image", "args": {}}, "thoughtSignature": "sig"},
                {"text": "", "thoughtSignature": "text-sig"}
            ]}),
            json!({"role": "user", "parts": [{"functionResponse": {
                "name": "view_image",
                "id": "image-call",
                "response": {"output": ["See this", {"$ref": "image-0"}, "End of result"]},
                "parts": [{"inlineData": {"mimeType": "image/png", "data": "YQ==", "displayName": "image-0"}}]
            }}]}),
        ]
    );
    Ok(())
}
