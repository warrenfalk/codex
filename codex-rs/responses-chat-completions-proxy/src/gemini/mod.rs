//! Native Gemini request and stream translation. The public REST transport only
//! adds the endpoint and credential to the translated request.

mod history;
mod stream;

use crate::config::BackendCapabilities;
use crate::config::ProxyConfig;
use crate::error::ProxyError;
use crate::protocol::ResponsesRequest;
use crate::request::validate_request_controls;
use crate::tool_registry::ToolRegistry;
use crate::upstream::read_bounded_body;
use axum::body::Body;
use http::HeaderValue;
use http::StatusCode;
use http::header::ACCEPT;
use http::header::CONTENT_TYPE;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use tokio::time::timeout;

const REPLAY_PREFIX: &str = "codex-gemini-generate-content-v1:";
const MAX_REPLAY_BYTES: usize = 32 * 1024;
const MAX_STREAM_EVENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 64 * 1024;

/// Gives native parts their original order without duplicating visible text or
/// arguments in the rollout. Signatures are replayed as opaque strings.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReplayPart {
    Text {
        bytes: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    Function {
        call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        omit_provider_id: bool,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentRequest {
    contents: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<Value>,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    generation_config: serde_json::Map<String, Value>,
}

pub(crate) async fn respond(
    client: &reqwest::Client,
    config: &ProxyConfig,
    request: ResponsesRequest,
) -> Result<Body, ProxyError> {
    let (request, tools) = translate_request(request)?;
    let key = config
        .upstream_bearer
        .as_deref()
        .ok_or_else(|| ProxyError::upstream("Gemini API key is missing"))?;
    let mut credential = HeaderValue::from_str(key)
        .map_err(|_| ProxyError::upstream("Gemini API key is not a valid header value"))?;
    credential.set_sensitive(true);
    let response = timeout(
        config.request_timeout,
        client
            .post(&config.upstream_url)
            .header(ACCEPT, "text/event-stream")
            .header("x-goog-api-key", credential)
            .json(&request)
            .send(),
    )
    .await
    .map_err(|_| ProxyError::upstream("timeout waiting for Gemini response headers"))?
    .map_err(|error| ProxyError::upstream(error.without_url().to_string()))?;
    let status = response.status();
    if !status.is_success() {
        let message = timeout(
            config.stream_idle_timeout,
            read_bounded_body(response, MAX_ERROR_BYTES),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(|message| {
                    message
                        .replace(key, "[redacted]")
                        .chars()
                        .take(4096)
                        .collect::<String>()
                })
        })
        .unwrap_or_else(|| format!("Gemini returned {status}"));
        if status == StatusCode::NOT_FOUND {
            return Err(ProxyError::ModelNotFound {
                status: Some(status),
                message,
            });
        }
        return Err(ProxyError::upstream_http(status, message));
    }
    if !response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|mime| mime.starts_with("text/event-stream"))
    {
        return Err(ProxyError::invalid_upstream(
            "Gemini did not return an SSE stream",
        ));
    }
    Ok(Body::from_stream(stream::translate(
        response,
        tools,
        config.stream_idle_timeout,
    )))
}

fn translate_request(
    request: ResponsesRequest,
) -> Result<(GenerateContentRequest, ToolRegistry), ProxyError> {
    validate_request_controls(
        &request,
        BackendCapabilities {
            reasoning_effort: true,
            parallel_tool_calls: true,
            ..BackendCapabilities::default()
        },
    )?;
    let tools = ToolRegistry::from_responses_tools(request.tools)?;
    let history = history::compile(request.instructions, request.input, &tools)?;
    let declarations = tools.chat_tools().map(|tools| {
        vec![json!({
            "functionDeclarations": tools.iter().map(|tool| {
                let function = &tool["function"];
                let mut declaration = json!({
                    "name": function["name"],
                    "description": function["description"],
                });
                if !function["parameters"].is_null() {
                    declaration["parametersJsonSchema"] = function["parameters"].clone();
                }
                declaration
            }).collect::<Vec<_>>()
        })]
    });
    let mode = match request.tool_choice.as_str() {
        Some("auto") => "AUTO",
        Some("none") => "NONE",
        Some("required") if declarations.is_some() => "ANY",
        Some("required") => return Err(ProxyError::invalid("tool_choice=required requires tools")),
        _ => return Err(ProxyError::unsupported("Gemini tool_choice")),
    };
    let tool_config = declarations
        .as_ref()
        .map(|_| json!({"functionCallingConfig": {"mode": mode}}));
    let mut generation_config = serde_json::Map::new();
    if let Some(effort) = request.reasoning.and_then(|reasoning| reasoning.effort) {
        let level = match effort.as_str() {
            "none" => None,
            "minimal" | "low" => Some("LOW"),
            "medium" => Some("MEDIUM"),
            "high" => Some("HIGH"),
            "xhigh" => Some("XHIGH"),
            _ => {
                return Err(ProxyError::unsupported(format!(
                    "Gemini reasoning effort {effort:?}"
                )));
            }
        };
        if let Some(level) = level {
            generation_config.insert(
                "thinkingConfig".to_string(),
                json!({"thinkingLevel": level}),
            );
        }
    }
    if let Some(text) = request.text {
        if text.verbosity.is_some() {
            return Err(ProxyError::unsupported("Gemini text.verbosity"));
        }
        if let Some(format) = text.format {
            if format.kind != "json_schema" {
                return Err(ProxyError::unsupported(format!(
                    "Gemini text format {:?}",
                    format.kind
                )));
            }
            generation_config.insert("responseMimeType".to_string(), json!("application/json"));
            generation_config.insert("responseJsonSchema".to_string(), format.schema);
        }
    }
    Ok((
        GenerateContentRequest {
            contents: history.contents,
            system_instruction: (!history.instructions.is_empty())
                .then(|| json!({"parts": history.instructions})),
            tools: declarations,
            tool_config,
            generation_config,
        },
        tools,
    ))
}

#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tests;
