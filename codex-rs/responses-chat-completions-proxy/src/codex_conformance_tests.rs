use super::*;
use axum::extract::State;
use axum::routing::post;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::config_types::ReasoningSummary;
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
struct ScriptedChat {
    requests: Arc<Mutex<Vec<Value>>>,
}

#[derive(Clone, Default)]
struct ScriptedCustomChat {
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn scripted_chat(State(state): State<ScriptedChat>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let request_index = if let Ok(mut requests) = state.requests.lock() {
        let request_index = requests.len();
        requests.push(request.clone());
        request_index
    } else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "script request lock poisoned",
        )
            .into_response();
    };

    if request_index == 0 {
        let offered_names = request["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        let (name, first_arguments, second_arguments) = if offered_names.contains(&"update_plan") {
            (
                "update_plan",
                "{\"explanation\":\"adapter test\",\"plan\":[{\"step\":\"Check adap",
                "ter\",\"status\":\"completed\"}]}",
            )
        } else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Codex offered no update_plan tool: {offered_names:?}"),
            )
                .into_response();
        };
        return chat_sse(vec![
            json!({
                "id": "chatcmpl_tool",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "reasoning_content": "Need two ",
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_plan_1",
                                "type": "function",
                                "function": {"name": name, "arguments": first_arguments}
                            },
                            {
                                "index": 1,
                                "id": "call_plan_2",
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": "{\"plan\":[{\"step\":\"Check parallel calls\",\"status\":\"completed\"}]}"
                                }
                            }
                        ]
                    },
                    "finish_reason": null
                }]
            }),
            json!({
                "id": "chatcmpl_tool",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "reasoning_content": "plan calls.",
                        "tool_calls": [{
                            "index": 0,
                            "function": {"arguments": second_arguments}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }),
        ]);
    }

    chat_sse(vec![
        json!({
            "id": "chatcmpl_final",
            "choices": [{
                "index": 0,
                "delta": {"content": "tool "},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl_final",
            "choices": [{
                "index": 0,
                "delta": {"content": "complete"},
                "finish_reason": "stop"
            }]
        }),
        json!({
            "id": "chatcmpl_final",
            "choices": [],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 2,
                "total_tokens": 22
            }
        }),
    ])
}

fn chat_sse(chunks: Vec<Value>) -> Response {
    let mut body = chunks
        .into_iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect::<String>();
    body.push_str("data: [DONE]\n\n");
    ([(CONTENT_TYPE, "text/event-stream")], body).into_response()
}

async fn scripted_custom_chat(State(state): State<ScriptedCustomChat>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let request_index = if let Ok(mut requests) = state.requests.lock() {
        let request_index = requests.len();
        requests.push(request.clone());
        request_index
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if request_index > 0 {
        return chat_sse(vec![json!({
            "id": "chatcmpl_custom_final",
            "choices": [{
                "index": 0,
                "delta": {"content": "patch complete"},
                "finish_reason": "stop"
            }]
        })]);
    }
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
    let patch = "*** Begin Patch\n*** Add File: adapter-proof.txt\n+created\n*** End Patch";
    let arguments = json!({"input": patch}).to_string();
    chat_sse(vec![json!({
        "id": "chatcmpl_custom",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_patch",
                    "type": "function",
                    "function": {"name": custom_name, "arguments": arguments}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    })])
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
async fn unmodified_codex_round_trips_parallel_tool_calls_through_chat() -> anyhow::Result<()> {
    let chat = ScriptedChat::default();
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(scripted_chat))
            .with_state(chat.clone()),
    )
    .await?;
    let proxy = spawn_server(router(ProxyConfig {
        listen_address: "127.0.0.1".parse()?,
        port: 0,
        upstream_url: format!("http://{upstream}/v1/chat/completions"),
        upstream_model: Some("scripted-chat".to_string()),
        upstream_bearer: None,
        forward_inbound_authorization: false,
        server_info: None,
        request_timeout: Duration::from_secs(5),
        stream_idle_timeout: Duration::from_secs(5),
        capabilities: BackendCapabilities {
            parallel_tool_calls: true,
            prompt_cache_key: PromptCacheKeyPolicy::Forward,
            reasoning_content: ReasoningContentPolicy::Plaintext,
            reasoning_effort: true,
            ..BackendCapabilities::default()
        },
    })?)
    .await?;
    let mut provider =
        built_in_model_providers(/* openai_base_url */ /*openai_base_url*/ None)["openai"].clone();
    provider.name = "scripted-chat-proxy".to_string();
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
            config.base_instructions = Some("Use the requested tool, then answer.".to_string());
            config.model_provider = provider;
            config.codex_self_exe = Some(current_exe);
            config.update_plan_enabled = true;
            assert!(config.web_search_mode.set(WebSearchMode::Disabled).is_ok());
        })
        .with_model_info_override("gpt-5.5", |model| {
            model.shell_type = ConfigShellToolType::Disabled;
            model.supports_reasoning_summary_parameter = true;
            model.default_reasoning_summary = ReasoningSummary::None;
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
                text: "Update the plan and report completion.".to_string(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(SandboxPolicy::DangerFullAccess),
                ..Default::default()
            }),
        )
        .await?;

    let mut streamed = String::new();
    let mut final_message = None;
    loop {
        let event = timeout(Duration::from_secs(20), codex.next_event()).await??;
        match event.msg {
            EventMsg::AgentMessageContentDelta(delta) => streamed.push_str(&delta.delta),
            EventMsg::AgentMessage(message) => final_message = Some(message.message),
            EventMsg::Error(error) => anyhow::bail!("Codex turn failed: {}", error.message),
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }

    assert!(streamed.contains("tool complete"));
    assert_eq!(final_message.as_deref(), Some("tool complete"));
    let requests = chat
        .requests
        .lock()
        .map_err(|error| anyhow::anyhow!("script request lock poisoned: {error}"))?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["model"], json!("scripted-chat"));
    assert_eq!(
        requests[0]["prompt_cache_key"],
        requests[1]["prompt_cache_key"]
    );
    assert!(
        requests[0]["prompt_cache_key"]
            .as_str()
            .is_some_and(|key| !key.is_empty())
    );
    assert!(
        requests[1]["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| {
                message["role"] == "assistant"
                    && message["reasoning_content"] == "Need two plan calls."
                    && message["tool_calls"][0]["id"] == "call_plan_1"
                    && message["tool_calls"][1]["id"] == "call_plan_2"
            }))
    );
    assert!(requests[1]["messages"].as_array().is_some_and(|messages| {
        messages
            .iter()
            .any(|message| message["role"] == "tool" && message["tool_call_id"] == "call_plan_1")
    }));
    assert!(requests[1]["messages"].as_array().is_some_and(|messages| {
        messages
            .iter()
            .any(|message| message["role"] == "tool" && message["tool_call_id"] == "call_plan_2")
    }));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unmodified_codex_executes_a_translated_custom_tool() -> anyhow::Result<()> {
    let chat = ScriptedCustomChat::default();
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(scripted_custom_chat))
            .with_state(chat.clone()),
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
    provider.name = "scripted-custom-chat-proxy".to_string();
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
            config.base_instructions = Some("Apply the requested patch, then answer.".to_string());
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
    let test_codex = builder
        .build_with_auto_env(&unused_responses_server)
        .await?;
    let codex = &test_codex.codex;
    codex
        .start_turn_if_idle(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Create adapter-proof.txt.".to_string(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(SandboxPolicy::DangerFullAccess),
                ..Default::default()
            }),
        )
        .await?;
    loop {
        let event = timeout(Duration::from_secs(20), codex.next_event()).await??;
        match event.msg {
            EventMsg::Error(error) => anyhow::bail!("Codex turn failed: {}", error.message),
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }

    assert_eq!(
        std::fs::read_to_string(test_codex.cwd_path().join("adapter-proof.txt"))?,
        "created\n"
    );
    let requests = chat
        .requests
        .lock()
        .map_err(|error| anyhow::anyhow!("custom script request lock poisoned: {error}"))?;
    assert_eq!(requests.len(), 2);
    assert!(requests[1]["messages"].as_array().is_some_and(|messages| {
        messages
            .iter()
            .any(|message| message["role"] == "tool" && message["tool_call_id"] == "call_patch")
    }));
    Ok(())
}
