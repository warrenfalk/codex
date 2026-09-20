mod config;
mod error;
mod history;
mod protocol;
mod request;
mod stream;
mod tool_registry;
mod upstream;

pub use config::Args;
pub use config::BackendCapabilities;
pub use config::PromptCacheKeyPolicy;
pub use config::ProxyConfig;
pub use config::ReasoningContentPolicy;

use anyhow::Context;
use axum::Router;
use axum::body::Body;
use axum::body::Bytes;
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use error::ProxyError;
use protocol::ResponsesRequest;
use reqwest::Client;
use serde::Serialize;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use stream::translate_upstream;

const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct EmbeddedProxy {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl EmbeddedProxy {
    pub fn base_url(&self) -> String {
        format!("http://{}/v1", self.address)
    }
}

impl Drop for EmbeddedProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
struct AppState {
    config: Arc<ProxyConfig>,
    client: Client,
}

#[derive(Serialize)]
struct ServerInfo {
    port: u16,
    pid: u32,
}

pub async fn serve(config: ProxyConfig) -> anyhow::Result<()> {
    let address = SocketAddr::new(config.listen_address, config.port);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind {address}"))?;
    let bound_address = listener
        .local_addr()
        .context("failed to read bound address")?;
    eprintln!("responses-chat-completions-proxy listening on {bound_address}");
    if let Some(path) = config.server_info.as_deref() {
        write_server_info(path, bound_address.port())?;
    }
    let app = router(config)?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("proxy server failed")
}

pub fn start_embedded(config: ProxyConfig) -> anyhow::Result<EmbeddedProxy> {
    let runtime = tokio::runtime::Handle::try_current()
        .context("the embedded proxy requires an active Tokio runtime")?;
    let address = SocketAddr::new(config.listen_address, config.port);
    let listener = std::net::TcpListener::bind(address)
        .with_context(|| format!("failed to bind {address}"))?;
    listener
        .set_nonblocking(true)
        .context("failed to configure embedded proxy listener")?;
    let listener = tokio::net::TcpListener::from_std(listener)
        .context("failed to attach embedded proxy listener to the Tokio runtime")?;
    let address = listener
        .local_addr()
        .context("failed to read embedded proxy address")?;
    let app = router(config)?;
    let task = runtime.spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("embedded responses-chat-completions proxy failed: {error}");
        }
    });
    Ok(EmbeddedProxy { address, task })
}

pub fn router(config: ProxyConfig) -> anyhow::Result<Router> {
    if config.upstream_bearer.is_some() && config.forward_inbound_authorization {
        anyhow::bail!(
            "proxy-owned credentials and inbound Authorization forwarding are mutually exclusive"
        );
    }
    let upstream_url =
        reqwest::Url::parse(&config.upstream_url).context("--upstream-url is not a valid URL")?;
    if !matches!(upstream_url.scheme(), "http" | "https") {
        anyhow::bail!("--upstream-url must use http or https");
    }
    if !upstream_url.username().is_empty() || upstream_url.password().is_some() {
        anyhow::bail!("--upstream-url must not contain credentials");
    }
    let client = Client::builder()
        .connect_timeout(config.request_timeout)
        .build()
        .context("failed to build upstream HTTP client")?;
    let state = AppState {
        config: Arc::new(config),
        client,
    };
    Ok(Router::new()
        .route("/v1/responses", post(handle_responses))
        .route("/healthz", get(health))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state))
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn handle_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ProxyError> {
    let request: ResponsesRequest = serde_json::from_slice(&body)
        .map_err(|error| ProxyError::invalid(format!("invalid JSON body: {error}")))?;
    let translated = request::translate_request(
        request,
        state.config.capabilities,
        state.config.upstream_model.as_deref(),
    )?;
    let upstream =
        upstream::send_chat_request(&state.client, &state.config, &headers, &translated.chat)
            .await?;
    let body = Body::from_stream(translate_upstream(
        upstream,
        translated.tools,
        state.config.stream_idle_timeout,
        state.config.capabilities.reasoning_content,
    ));
    Ok((
        [
            (CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn write_server_info(path: &Path, port: u16) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut data = serde_json::to_string(&ServerInfo {
        port,
        pid: std::process::id(),
    })?;
    data.push('\n');
    let mut file = File::create(path)?;
    file.write_all(data.as_bytes())?;
    Ok(())
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "codex_conformance_tests.rs"]
mod codex_conformance_tests;

#[cfg(test)]
#[path = "codex_structured_conformance_tests.rs"]
mod codex_structured_conformance_tests;

#[cfg(test)]
#[path = "codex_resilience_conformance_tests.rs"]
mod codex_resilience_conformance_tests;

#[cfg(test)]
#[path = "stream_failure_integration_tests.rs"]
mod stream_failure_integration_tests;

#[cfg(test)]
#[path = "codex_tool_error_conformance_tests.rs"]
mod codex_tool_error_conformance_tests;
