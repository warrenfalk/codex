use crate::config::BackendCapabilities;
use crate::config::PromptCacheKeyPolicy;
use crate::error::ProxyError;
use crate::history::compile_history;
use crate::protocol::ChatRequest;
use crate::protocol::ChatStreamOptions;
use crate::protocol::ResponsesRequest;
use crate::tool_registry::ToolRegistry;
use serde_json::Value;
use serde_json::json;

pub(crate) struct TranslatedRequest {
    pub chat: ChatRequest,
    pub tools: ToolRegistry,
}

pub(crate) fn translate_request(
    request: ResponsesRequest,
    capabilities: BackendCapabilities,
    upstream_model: Option<&str>,
) -> Result<TranslatedRequest, ProxyError> {
    validate_request_controls(&request, capabilities)?;
    let tools = ToolRegistry::from_responses_tools(request.tools)?;
    let messages = compile_history(request.instructions, request.input, &tools, capabilities)?;
    let response_format = translate_text_controls(request.text, capabilities)?;
    let reasoning_effort = request.reasoning.and_then(|reasoning| reasoning.effort);
    let prompt_cache_key = match capabilities.prompt_cache_key {
        PromptCacheKeyPolicy::Omit => None,
        PromptCacheKeyPolicy::Forward => request.prompt_cache_key,
    };
    let chat = ChatRequest {
        model: upstream_model.unwrap_or(&request.model).to_string(),
        messages,
        tools: tools.chat_tools(),
        tool_choice: request.tool_choice,
        parallel_tool_calls: request.parallel_tool_calls,
        stream: capabilities.streaming,
        stream_options: capabilities.streaming.then_some(ChatStreamOptions {
            include_usage: true,
        }),
        response_format,
        reasoning_effort,
        prompt_cache_key,
    };
    Ok(TranslatedRequest { chat, tools })
}

fn validate_request_controls(
    request: &ResponsesRequest,
    capabilities: BackendCapabilities,
) -> Result<(), ProxyError> {
    if request.model.trim().is_empty() {
        return Err(ProxyError::invalid("model must not be empty"));
    }
    if !request.stream {
        return Err(ProxyError::unsupported(
            "stream=false is outside the Codex V1 compatibility profile",
        ));
    }
    let tool_choice = request
        .tool_choice
        .as_str()
        .ok_or_else(|| ProxyError::unsupported("non-string tool_choice"))?;
    if !matches!(tool_choice, "auto" | "none" | "required") {
        return Err(ProxyError::unsupported(format!(
            "tool_choice {tool_choice:?}"
        )));
    }
    if request.parallel_tool_calls && !capabilities.parallel_tool_calls {
        return Err(ProxyError::unsupported(
            "parallel_tool_calls=true; start the proxy with --supports-parallel-tool-calls or disable it in the model profile",
        ));
    }
    if request.stream_options.is_some() {
        return Err(ProxyError::unsupported(
            "Responses stream_options (including concurrent reasoning summaries)",
        ));
    }
    let unsupported_include = request
        .include
        .iter()
        .filter(|entry| entry.as_str() != "reasoning.encrypted_content");
    let unsupported_include = unsupported_include.cloned().collect::<Vec<_>>();
    if !unsupported_include.is_empty() {
        return Err(ProxyError::unsupported(format!(
            "Responses include entries {unsupported_include:?}"
        )));
    }
    if let Some(reasoning) = &request.reasoning {
        if reasoning.effort.is_some() && !capabilities.reasoning_effort {
            return Err(ProxyError::unsupported(
                "reasoning.effort; start the proxy with --supports-reasoning-effort or use a non-reasoning model profile",
            ));
        }
        if reasoning.summary.is_some()
            && capabilities.reasoning_content != crate::config::ReasoningContentPolicy::Plaintext
        {
            return Err(ProxyError::unsupported("reasoning summaries"));
        }
        if reasoning.context.is_some() {
            return Err(ProxyError::unsupported("reasoning context replay controls"));
        }
    }

    // These fields do not change the model-visible request in the V1 profile.
    let _ = (
        request.store,
        &request.service_tier,
        &request.client_metadata,
    );
    Ok(())
}

fn translate_text_controls(
    text: Option<crate::protocol::TextControls>,
    capabilities: BackendCapabilities,
) -> Result<Option<Value>, ProxyError> {
    let Some(text) = text else {
        return Ok(None);
    };
    if let Some(verbosity) = text.verbosity {
        return Err(ProxyError::unsupported(format!(
            "text.verbosity={verbosity:?}"
        )));
    }
    let Some(format) = text.format else {
        return Ok(None);
    };
    if !capabilities.structured_output {
        return Err(ProxyError::unsupported(
            "text.format; start the proxy with --supports-structured-output or disable output schemas",
        ));
    }
    if format.kind != "json_schema" {
        return Err(ProxyError::unsupported(format!(
            "text.format.type={:?}",
            format.kind
        )));
    }
    Ok(Some(json!({
        "type": "json_schema",
        "json_schema": {
            "name": format.name,
            "strict": format.strict,
            "schema": format.schema,
        }
    })))
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
