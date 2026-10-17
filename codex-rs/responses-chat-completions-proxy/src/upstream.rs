use crate::config::ProxyConfig;
use crate::error::ProxyError;
use crate::protocol::BufferedChatCompletion;
use crate::protocol::ChatRequest;
use crate::stream::UpstreamResponseBody;
use futures::StreamExt;
use http::HeaderMap;
use http::header::ACCEPT;
use http::header::AUTHORIZATION;
use http::header::CONTENT_TYPE;
use reqwest::Client;
use tokio::time::timeout;

const MAX_BUFFERED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ERROR_RESPONSE_BYTES: usize = 64 * 1024;

pub(crate) async fn send_chat_request(
    client: &Client,
    config: &ProxyConfig,
    inbound_headers: &HeaderMap,
    request: &ChatRequest,
) -> Result<UpstreamResponseBody, ProxyError> {
    let accept = if request.stream {
        "text/event-stream"
    } else {
        "application/json"
    };
    let mut builder = client
        .post(&config.upstream_url)
        .header(ACCEPT, accept)
        .json(request);
    if let Some(bearer) = &config.upstream_bearer {
        builder = builder.bearer_auth(bearer);
    } else if config.forward_inbound_authorization
        && let Some(authorization) = inbound_headers.get(AUTHORIZATION)
    {
        builder = builder.header(AUTHORIZATION, authorization);
    }
    let response = timeout(config.request_timeout, builder.send())
        .await
        .map_err(|_| ProxyError::upstream("timeout waiting for upstream response headers"))?
        .map_err(|error| ProxyError::upstream(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        let message = upstream_error_message(response).await;
        return Err(ProxyError::upstream_http(status, message));
    }
    let is_event_stream = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));
    if is_event_stream {
        return Ok(UpstreamResponseBody::EventStream(response));
    }
    let bytes = read_bounded_body(response, MAX_BUFFERED_RESPONSE_BYTES).await?;
    let completion = serde_json::from_slice::<BufferedChatCompletion>(&bytes).map_err(|error| {
        ProxyError::invalid_upstream(format!(
            "successful non-SSE response is not a Chat completion: {error}"
        ))
    })?;
    Ok(UpstreamResponseBody::Buffered(completion))
}

async fn upstream_error_message(response: reqwest::Response) -> String {
    let status = response.status();
    let Ok(bytes) = read_bounded_body(response, MAX_ERROR_RESPONSE_BYTES).await else {
        return format!("{status} with an unreadable or oversized error body");
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return format!("{status} with a non-JSON error body");
    };
    value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(serde_json::Value::as_str)
        .map(|message| truncate(message, 4096))
        .unwrap_or_else(|| format!("{status} with no error message"))
}

async fn read_bounded_body(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, ProxyError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| ProxyError::upstream(error.to_string()))?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ProxyError::invalid_upstream(format!(
                "upstream response body exceeds the {max_bytes}-byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn truncate(message: &str, max_chars: usize) -> String {
    let mut chars = message.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}
