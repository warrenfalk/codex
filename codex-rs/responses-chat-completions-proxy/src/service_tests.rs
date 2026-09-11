use super::*;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Option<(HeaderMap, Value)>>>);

async fn streaming_chat(
    State(capture): State<Capture>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if let Ok(mut slot) = capture.0.lock() {
        *slot = Some((headers, value));
    }
    let data = [
        json!({
            "id": "chatcmpl_test",
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": "hello"},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl_test",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        }),
        json!({
            "id": "chatcmpl_test",
            "choices": [],
            "usage": {
                "prompt_tokens": 4,
                "completion_tokens": 1,
                "total_tokens": 5
            }
        }),
    ]
    .into_iter()
    .map(|chunk| format!("data: {chunk}\n\n"))
    .collect::<String>();
    (
        [(CONTENT_TYPE, "text/event-stream")],
        format!("{data}data: [DONE]\n\n"),
    )
        .into_response()
}

async fn rate_limited_chat() -> impl IntoResponse {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({"error": {"message": "try later", "secret": "not reflected"}})),
    )
}

async fn buffered_chat(State(capture): State<Capture>, body: Bytes) -> impl IntoResponse {
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if let Ok(mut slot) = capture.0.lock() {
        *slot = Some((HeaderMap::new(), value));
    }
    Json(json!({
        "id": "chatcmpl_buffered",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "buffered"},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 2,
            "completion_tokens": 1,
            "total_tokens": 3
        }
    }))
}

struct DropNotify {
    dropped: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Drop for DropNotify {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

#[derive(Clone)]
struct HangingState {
    dropped: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

async fn hanging_chat(State(state): State<HangingState>) -> Response {
    let body = Body::from_stream(async_stream::stream! {
        let _drop_notify = DropNotify {
            dropped: Arc::clone(&state.dropped),
            notify: Arc::clone(&state.notify),
        };
        let chunk = json!({
            "id": "chatcmpl_hanging",
            "choices": [{
                "index": 0,
                "delta": {"content": "started"},
                "finish_reason": null
            }]
        });
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(format!("data: {chunk}\n\n")));
        std::future::pending::<()>().await;
    });
    ([(CONTENT_TYPE, "text/event-stream")], body).into_response()
}

async fn spawn_server(app: Router) -> anyhow::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(address)
}

fn proxy_config(upstream_url: String) -> ProxyConfig {
    ProxyConfig {
        listen_address: "127.0.0.1"
            .parse()
            .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        port: 0,
        upstream_url,
        upstream_model: Some("backend-model".to_string()),
        upstream_bearer: None,
        forward_inbound_authorization: true,
        server_info: None,
        request_timeout: Duration::from_secs(5),
        stream_idle_timeout: Duration::from_secs(5),
        capabilities: BackendCapabilities::default(),
    }
}

fn responses_body() -> Value {
    json!({
        "model": "codex-model",
        "instructions": "Answer precisely.",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "Say hello"}]
        }],
        "tools": [],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "reasoning": null,
        "store": false,
        "stream": true,
        "include": [],
        "prompt_cache_key": "ignored",
        "client_metadata": {"trace": "ignored"}
    })
}

#[tokio::test]
async fn translates_a_complete_http_exchange() -> anyhow::Result<()> {
    let capture = Capture::default();
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(streaming_chat))
            .with_state(capture.clone()),
    )
    .await?;
    let proxy = spawn_server(router(proxy_config(format!(
        "http://{upstream}/v1/chat/completions"
    )))?)
    .await?;

    let response = reqwest::Client::new()
        .post(format!("http://{proxy}/v1/responses"))
        .bearer_auth("forward-me")
        .json(&responses_body())
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
    );
    let body = response.text().await?;
    let events = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(events[0]["type"], json!("response.created"));
    assert_eq!(
        events
            .iter()
            .find(|event| event["type"] == "response.output_item.done")
            .map(|event| &event["item"]["content"][0]["text"]),
        Some(&json!("hello"))
    );
    assert_eq!(
        events.last().map(|event| &event["type"]),
        Some(&json!("response.completed"))
    );

    let guard = capture
        .0
        .lock()
        .map_err(|error| anyhow::anyhow!("capture mutex poisoned: {error}"))?;
    let (headers, chat) = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("upstream request was not captured"))?;
    assert_eq!(
        headers
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer forward-me")
    );
    assert_eq!(chat["model"], json!("backend-model"));
    assert_eq!(chat["messages"][0]["role"], json!("system"));
    assert_eq!(chat["stream"], json!(true));
    assert_eq!(chat["stream_options"], json!({"include_usage": true}));
    Ok(())
}

#[tokio::test]
async fn proxy_owned_authorization_overrides_the_inbound_header() -> anyhow::Result<()> {
    let capture = Capture::default();
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(streaming_chat))
            .with_state(capture.clone()),
    )
    .await?;
    let mut config = proxy_config(format!("http://{upstream}/v1/chat/completions"));
    config.forward_inbound_authorization = false;
    config.upstream_bearer = Some("proxy-secret".to_string());
    let proxy = spawn_server(router(config)?).await?;

    let response = reqwest::Client::new()
        .post(format!("http://{proxy}/v1/responses"))
        .bearer_auth("do-not-forward")
        .json(&responses_body())
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.text().await?;
    let guard = capture
        .0
        .lock()
        .map_err(|error| anyhow::anyhow!("capture mutex poisoned: {error}"))?;
    let (headers, _) = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("upstream request was not captured"))?;
    assert_eq!(
        headers
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer proxy-secret")
    );
    Ok(())
}

#[tokio::test]
async fn preserves_upstream_http_status_and_only_exposes_the_error_message() -> anyhow::Result<()> {
    let upstream =
        spawn_server(Router::new().route("/v1/chat/completions", post(rate_limited_chat))).await?;
    let proxy = spawn_server(router(proxy_config(format!(
        "http://{upstream}/v1/chat/completions"
    )))?)
    .await?;

    let response = reqwest::Client::new()
        .post(format!("http://{proxy}/v1/responses"))
        .json(&responses_body())
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let body: Value = response.json().await?;
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("try later"))
    );
    assert!(!body.to_string().contains("not reflected"));
    Ok(())
}

#[tokio::test]
async fn dropping_the_downstream_response_cancels_the_upstream_body() -> anyhow::Result<()> {
    let state = HangingState {
        dropped: Arc::new(AtomicBool::new(false)),
        notify: Arc::new(Notify::new()),
    };
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(hanging_chat))
            .with_state(state.clone()),
    )
    .await?;
    let proxy = spawn_server(router(proxy_config(format!(
        "http://{upstream}/v1/chat/completions"
    )))?)
    .await?;
    let response = reqwest::Client::new()
        .post(format!("http://{proxy}/v1/responses"))
        .json(&responses_body())
        .send()
        .await?;
    let mut body = response.bytes_stream();
    let mut saw_delta = false;
    for _ in 0..4 {
        let chunk = timeout(Duration::from_secs(2), body.next())
            .await?
            .ok_or_else(|| anyhow::anyhow!("proxy response ended before a text delta"))??;
        if String::from_utf8_lossy(&chunk).contains("response.output_text.delta") {
            saw_delta = true;
            break;
        }
    }
    assert!(saw_delta, "proxy never forwarded the upstream text delta");

    drop(body);
    if !state.dropped.load(Ordering::SeqCst) {
        timeout(Duration::from_secs(2), state.notify.notified()).await?;
    }
    assert!(state.dropped.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn converts_a_declared_non_streaming_chat_backend_to_responses_sse() -> anyhow::Result<()> {
    let capture = Capture::default();
    let upstream = spawn_server(
        Router::new()
            .route("/v1/chat/completions", post(buffered_chat))
            .with_state(capture.clone()),
    )
    .await?;
    let mut config = proxy_config(format!("http://{upstream}/v1/chat/completions"));
    config.capabilities.streaming = false;
    let proxy = spawn_server(router(config)?).await?;

    let response = reqwest::Client::new()
        .post(format!("http://{proxy}/v1/responses"))
        .json(&responses_body())
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await?;
    assert!(body.contains("response.output_text.delta"));
    assert!(body.contains("response.completed"));
    assert!(body.contains("buffered"));
    let guard = capture
        .0
        .lock()
        .map_err(|error| anyhow::anyhow!("capture mutex poisoned: {error}"))?;
    let chat = guard
        .as_ref()
        .map(|(_, chat)| chat)
        .ok_or_else(|| anyhow::anyhow!("upstream request was not captured"))?;
    assert_eq!(chat["stream"], json!(false));
    assert!(chat.get("stream_options").is_none());
    Ok(())
}
