use crate::BackendCapabilities;
use crate::ProxyConfig;
use crate::ReasoningContentPolicy;
use crate::start_embedded;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::test_codex::test_codex;
use image::DynamicImage;
use image::ImageBuffer;
use image::Rgba;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::io::Cursor;
use std::time::Duration;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn codex_continues_after_view_image_and_replays_it_on_the_next_turn() -> anyhow::Result<()> {
    let upstream = MockServer::start().await;
    let call = json!({"id": "image_call", "type": "function", "function": {
        "name": "view_image", "arguments": "{\"path\":\"fixture.png\"}"
    }});
    let mut delta_call = call.clone();
    delta_call["index"] = json!(0);
    let tool_chunk = json!({"id": "chat_image", "choices": [{"index": 0, "delta": {
        "reasoning_content": "Inspect the image.", "tool_calls": [delta_call]
    }, "finish_reason": "tool_calls"}]});
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("data: {tool_chunk}\n\ndata: [DONE]\n\n"),
            "text/event-stream",
        ))
        .with_priority(1)
        .up_to_n_times(1)
        .mount(&upstream)
        .await;
    let final_chunk = json!({"id": "chat_final", "choices": [{"index": 0,
        "delta": {"content": "Image inspected."}, "finish_reason": "stop"
    }]});
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("data: {final_chunk}\n\ndata: [DONE]\n\n"),
            "text/event-stream",
        ))
        .with_priority(2)
        .mount(&upstream)
        .await;
    let proxy = start_embedded(ProxyConfig {
        listen_address: "127.0.0.1".parse()?,
        port: 0,
        upstream_url: format!("{}/v1/chat/completions", upstream.uri()),
        upstream_model: None,
        upstream_bearer: None,
        forward_inbound_authorization: false,
        server_info: None,
        request_timeout: Duration::from_secs(10),
        stream_idle_timeout: Duration::from_secs(10),
        capabilities: BackendCapabilities {
            image_input: true,
            image_tool_output: true,
            parallel_tool_calls: true,
            reasoning_content: ReasoningContentPolicy::Plaintext,
            reasoning_effort: true,
            ..Default::default()
        },
    })?;
    let provider = ModelProviderInfo {
        name: "image-test".to_string(),
        base_url: Some(proxy.base_url()),
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        ..Default::default()
    };
    let mut builder = test_codex()
        .with_config(move |config| {
            config.model_provider = provider;
            config.web_search_mode.set(WebSearchMode::Disabled).unwrap();
        })
        .with_model_info_override("gpt-5.5", |model| {
            model.shell_type = ConfigShellToolType::Disabled;
            model.input_modalities = vec![InputModality::Text, InputModality::Image];
            model.supports_reasoning_summary_parameter = true;
            model.default_reasoning_summary = ReasoningSummary::None;
            model.support_verbosity = false;
            model.supports_search_tool = false;
            model.use_responses_lite = false;
        });
    let test = builder.build_with_auto_env(&upstream).await?;
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(ImageBuffer::from_pixel(32, 32, Rgba([255, 0, 0, 255])))
        .write_to(&mut png, image::ImageFormat::Png)?;
    test.fs()
        .write_file(
            &test.workspace_path_uri("fixture.png")?,
            png.into_inner(),
            Default::default(),
            /*sandbox*/ None,
        )
        .await?;

    for prompt in ["Inspect fixture.png.", "What did you see?"] {
        test.codex
            .start_turn_if_idle(
                TurnInputRequest::user_input(vec![UserInput::Text {
                    text: prompt.to_string(),
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
            match timeout(Duration::from_secs(20), test.codex.next_event())
                .await??
                .msg
            {
                EventMsg::AgentMessage(message) => final_message = Some(message.message),
                EventMsg::Error(error) => anyhow::bail!("Codex turn failed: {}", error.message),
                EventMsg::TurnComplete(_) => break,
                _ => {}
            }
        }
        assert_eq!(final_message.as_deref(), Some("Image inspected."));
    }
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    let mut replayed_tools = Vec::new();
    for request in &requests[1..] {
        let body: Value = request.body_json()?;
        let messages = body["messages"].as_array().unwrap();
        let assistant_index = messages
            .iter()
            .position(|message| message["tool_calls"][0]["id"] == "image_call")
            .unwrap();
        assert_eq!(
            messages[assistant_index],
            json!({"role": "assistant", "content": null,
            "reasoning_content": "Inspect the image.", "tool_calls": [call.clone()]})
        );
        let tool = &messages[assistant_index + 1];
        let image_url = tool["content"][0]["image_url"]["url"].as_str().unwrap();
        assert!(image_url.starts_with("data:image/"));
        let detail = tool["content"][0]["image_url"]["detail"].as_str().unwrap();
        assert!(matches!(detail, "high" | "original"));
        assert_eq!(
            tool,
            &json!({"role": "tool", "tool_call_id": "image_call", "content": [{
                "type": "image_url", "image_url": {"url": image_url, "detail": detail}
            }]})
        );
        replayed_tools.push(tool.clone());
    }
    assert_eq!(replayed_tools[0], replayed_tools[1]);
    Ok(())
}
