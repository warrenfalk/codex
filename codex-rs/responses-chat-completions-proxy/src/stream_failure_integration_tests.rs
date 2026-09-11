use super::*;
use axum::Json;
use axum::routing::post;
use futures::stream;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;

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
        upstream_model: None,
        upstream_bearer: None,
        forward_inbound_authorization: false,
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
        "stream": true
    })
}

async fn run_proxy_request(config: ProxyConfig) -> anyhow::Result<reqwest::Response> {
    let proxy = spawn_server(router(config)?).await?;
    Ok(reqwest::Client::new()
        .post(format!("http://{proxy}/v1/responses"))
        .json(&responses_body())
        .send()
        .await?)
}

async fn closed_early_chat() -> Response {
    let chunk = json!({
        "id": "chatcmpl_closed",
        "choices": [{
            "index": 0,
            "delta": {"content": "partial"},
            "finish_reason": null
        }]
    });
    (
        [(CONTENT_TYPE, "text/event-stream")],
        format!("data: {chunk}\n\n"),
    )
        .into_response()
}

async fn idle_chat() -> Response {
    let chunk = json!({
        "id": "chatcmpl_idle",
        "choices": [{
            "index": 0,
            "delta": {"content": "partial"},
            "finish_reason": null
        }]
    });
    let body = Body::from_stream(async_stream::stream! {
        yield Ok::<Bytes, Infallible>(Bytes::from(format!("data: {chunk}\n\n")));
        std::future::pending::<()>().await;
    });
    ([(CONTENT_TYPE, "text/event-stream")], body).into_response()
}

async fn delayed_headers_chat() -> Response {
    std::future::pending::<Response>().await
}

async fn fragmented_chat() -> Response {
    let first = json!({
        "id": "chatcmpl_fragmented",
        "choices": [{
            "index": 0,
            "delta": {"content": "fragmented"},
            "finish_reason": null
        }]
    });
    let second = json!({
        "id": "chatcmpl_fragmented",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    });
    let data = format!("data: {first}\n\ndata: {second}\n\ndata: [DONE]\n\n");
    let pieces = data
        .as_bytes()
        .chunks(7)
        .map(Bytes::copy_from_slice)
        .map(Ok::<_, Infallible>)
        .collect::<Vec<_>>();
    let body = Body::from_stream(stream::iter(pieces));
    ([(CONTENT_TYPE, "text/event-stream")], body).into_response()
}

async fn server_error_chat() -> impl IntoResponse {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": {"message": "backend unavailable"}})),
    )
}

#[tokio::test]
async fn early_stream_closure_becomes_a_failed_response() -> anyhow::Result<()> {
    let upstream =
        spawn_server(Router::new().route("/v1/chat/completions", post(closed_early_chat))).await?;

    let response = run_proxy_request(proxy_config(format!(
        "http://{upstream}/v1/chat/completions"
    )))
    .await?;
    let body = response.text().await?;

    assert!(body.contains("response.output_text.delta"));
    assert!(body.contains("response.failed"));
    assert!(body.contains("stream closed before a finish_reason"));
    assert!(!body.contains("response.completed"));
    Ok(())
}

#[tokio::test]
async fn stream_idle_timeout_becomes_a_failed_response() -> anyhow::Result<()> {
    let upstream =
        spawn_server(Router::new().route("/v1/chat/completions", post(idle_chat))).await?;
    let mut config = proxy_config(format!("http://{upstream}/v1/chat/completions"));
    config.stream_idle_timeout = Duration::from_millis(50);

    let response = run_proxy_request(config).await?;
    let body = response.text().await?;

    assert!(body.contains("response.failed"));
    assert!(body.contains("idle timeout waiting for Chat SSE"));
    Ok(())
}

#[tokio::test]
async fn response_header_timeout_is_a_responses_shaped_http_error() -> anyhow::Result<()> {
    let upstream =
        spawn_server(Router::new().route("/v1/chat/completions", post(delayed_headers_chat)))
            .await?;
    let mut config = proxy_config(format!("http://{upstream}/v1/chat/completions"));
    config.request_timeout = Duration::from_millis(50);

    let response = run_proxy_request(config).await?;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = response.json::<Value>().await?;
    assert_eq!(body["error"]["type"], json!("upstream_error"));
    assert!(
        body["error"]["message"].as_str().is_some_and(
            |message| message.contains("timeout waiting for upstream response headers")
        )
    );
    Ok(())
}

#[tokio::test]
async fn fragmented_sse_and_upstream_5xx_are_classified_correctly() -> anyhow::Result<()> {
    let fragmented =
        spawn_server(Router::new().route("/v1/chat/completions", post(fragmented_chat))).await?;
    let response = run_proxy_request(proxy_config(format!(
        "http://{fragmented}/v1/chat/completions"
    )))
    .await?;
    let body = response.text().await?;
    assert!(body.contains("fragmented"));
    assert!(body.contains("response.completed"));

    let failing =
        spawn_server(Router::new().route("/v1/chat/completions", post(server_error_chat))).await?;
    let response = run_proxy_request(proxy_config(format!(
        "http://{failing}/v1/chat/completions"
    )))
    .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.json::<Value>().await?;
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("backend unavailable"))
    );
    Ok(())
}
