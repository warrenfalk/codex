use crate::config::BackendCapabilities;
use crate::config::ReasoningContentPolicy;
use crate::error::ProxyError;
use crate::protocol::ChatFunctionCall;
use crate::protocol::ChatMessage;
use crate::protocol::ChatToolCall;
use crate::tool_registry::ToolIdentity;
use crate::tool_registry::ToolRegistry;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

#[derive(Default)]
struct PendingAssistant {
    reasoning_content: Vec<String>,
    text: Vec<String>,
    tool_calls: Vec<ChatToolCall>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryCallKind {
    Function,
    Custom,
}

struct HistoryCall {
    kind: HistoryCallKind,
    has_output: bool,
}

pub(crate) fn compile_history(
    instructions: String,
    input: Vec<Value>,
    tools: &ToolRegistry,
    capabilities: BackendCapabilities,
) -> Result<Vec<ChatMessage>, ProxyError> {
    let mut messages = Vec::new();
    let mut assistant = PendingAssistant::default();
    let mut calls = HashMap::new();
    if !instructions.is_empty() {
        push_control_message(&mut messages, instructions, capabilities);
    }

    for raw_item in input {
        let item_type = raw_item
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| ProxyError::invalid("input item is missing a string type"))?
            .to_string();
        if !matches!(
            item_type.as_str(),
            "message"
                | "agent_message"
                | "reasoning"
                | "function_call"
                | "function_call_output"
                | "custom_tool_call"
                | "custom_tool_call_output"
        ) {
            return Err(ProxyError::unsupported(format!(
                "input item type {item_type:?}"
            )));
        }
        let item: ResponseItem = serde_json::from_value(raw_item).map_err(|error| {
            ProxyError::invalid(format!("invalid {item_type} input item: {error}"))
        })?;
        match item {
            ResponseItem::Message { role, content, .. } => match role.as_str() {
                "assistant" => {
                    ensure_no_unresolved_calls(&calls, "an assistant message")?;
                    append_assistant_content(&mut assistant, content)?;
                }
                "user" => {
                    ensure_no_unresolved_calls(&calls, "a user message")?;
                    flush_assistant(&mut messages, &mut assistant);
                    messages.push(ChatMessage::User {
                        content: user_content(content, capabilities)?,
                    });
                }
                "developer" | "system" => {
                    ensure_no_unresolved_calls(&calls, "a control message")?;
                    flush_assistant(&mut messages, &mut assistant);
                    push_control_message(
                        &mut messages,
                        text_content(content, &role)?,
                        capabilities,
                    );
                }
                unsupported => {
                    return Err(ProxyError::unsupported(format!(
                        "message role {unsupported:?}"
                    )));
                }
            },
            ResponseItem::AgentMessage {
                author,
                recipient,
                content,
                ..
            } => {
                ensure_no_unresolved_calls(&calls, "an agent message")?;
                flush_assistant(&mut messages, &mut assistant);
                append_agent_message(&mut messages, author, recipient, content, capabilities)?;
            }
            ResponseItem::Reasoning {
                summary,
                content,
                encrypted_content,
                ..
            } => {
                if encrypted_content.is_some() {
                    return Err(ProxyError::unsupported("encrypted reasoning history"));
                }
                if !summary.is_empty() {
                    return Err(ProxyError::unsupported("reasoning summary history"));
                }
                let reasoning = plaintext_reasoning(content)?;
                if !reasoning.is_empty() {
                    if capabilities.reasoning_content != ReasoningContentPolicy::Plaintext {
                        return Err(ProxyError::unsupported(
                            "plaintext reasoning history; start the proxy with --supports-reasoning-content or use a profile that enables it",
                        ));
                    }
                    if !assistant.text.is_empty() || !assistant.tool_calls.is_empty() {
                        flush_assistant(&mut messages, &mut assistant);
                    }
                    assistant.reasoning_content.push(reasoning);
                }
            }
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                call_id,
                ..
            } => {
                register_call(&mut calls, &call_id, HistoryCallKind::Function)?;
                let identity = ToolIdentity::function(&name, namespace.as_deref());
                assistant.tool_calls.push(chat_tool_call(
                    call_id,
                    tools.safe_name(&identity),
                    arguments,
                ));
            }
            ResponseItem::CustomToolCall {
                name,
                namespace,
                input,
                call_id,
                ..
            } => {
                register_call(&mut calls, &call_id, HistoryCallKind::Custom)?;
                let identity = ToolIdentity::custom(&name, namespace.as_deref());
                assistant.tool_calls.push(chat_tool_call(
                    call_id,
                    tools.safe_name(&identity),
                    serde_json::to_string(&json!({"input": input})).map_err(|error| {
                        ProxyError::invalid(format!("failed to encode custom tool input: {error}"))
                    })?,
                ));
            }
            ResponseItem::FunctionCallOutput {
                call_id, output, ..
            } => {
                let call_id = call_id.ok_or_else(|| {
                    ProxyError::unsupported("function call output history without call_id")
                })?;
                resolve_call(&mut calls, &call_id, HistoryCallKind::Function)?;
                push_tool_output(&mut messages, &mut assistant, call_id, output)?;
            }
            ResponseItem::CustomToolCallOutput {
                call_id, output, ..
            } => {
                resolve_call(&mut calls, &call_id, HistoryCallKind::Custom)?;
                push_tool_output(&mut messages, &mut assistant, call_id, output)?;
            }
            _ => {
                return Err(ProxyError::unsupported(format!(
                    "input item decoded as {item:?}"
                )));
            }
        }
    }
    ensure_no_unresolved_calls(&calls, "the end of the request")?;
    flush_assistant(&mut messages, &mut assistant);
    Ok(messages)
}

fn register_call(
    calls: &mut HashMap<String, HistoryCall>,
    call_id: &str,
    kind: HistoryCallKind,
) -> Result<(), ProxyError> {
    if call_id.is_empty() {
        return Err(ProxyError::invalid("tool call has an empty call_id"));
    }
    if calls
        .insert(
            call_id.to_string(),
            HistoryCall {
                kind,
                has_output: false,
            },
        )
        .is_some()
    {
        return Err(ProxyError::invalid(format!(
            "duplicate tool call ID {call_id:?}"
        )));
    }
    Ok(())
}

fn resolve_call(
    calls: &mut HashMap<String, HistoryCall>,
    call_id: &str,
    expected_kind: HistoryCallKind,
) -> Result<(), ProxyError> {
    let call = calls.get_mut(call_id).ok_or_else(|| {
        ProxyError::invalid(format!("tool output refers to unknown call ID {call_id:?}"))
    })?;
    if call.kind != expected_kind {
        return Err(ProxyError::invalid(format!(
            "tool output kind does not match call ID {call_id:?}"
        )));
    }
    if call.has_output {
        return Err(ProxyError::invalid(format!(
            "tool call ID {call_id:?} has more than one output"
        )));
    }
    call.has_output = true;
    Ok(())
}

fn ensure_no_unresolved_calls(
    calls: &HashMap<String, HistoryCall>,
    next: &str,
) -> Result<(), ProxyError> {
    let mut unresolved = calls
        .iter()
        .filter(|(_, call)| !call.has_output)
        .map(|(call_id, _)| call_id.as_str())
        .collect::<Vec<_>>();
    unresolved.sort_unstable();
    if unresolved.is_empty() {
        return Ok(());
    }
    Err(ProxyError::invalid(format!(
        "unresolved tool calls before {next}: {}",
        unresolved.join(", ")
    )))
}

fn push_tool_output(
    messages: &mut Vec<ChatMessage>,
    assistant: &mut PendingAssistant,
    call_id: String,
    output: codex_protocol::models::FunctionCallOutputPayload,
) -> Result<(), ProxyError> {
    flush_assistant(messages, assistant);
    let content = output
        .text_content()
        .ok_or_else(|| ProxyError::unsupported("structured or image tool-call output history"))?;
    messages.push(ChatMessage::Tool {
        tool_call_id: call_id,
        content: content.to_string(),
    });
    Ok(())
}

fn push_control_message(
    messages: &mut Vec<ChatMessage>,
    content: String,
    capabilities: BackendCapabilities,
) {
    if capabilities.developer_role {
        messages.push(ChatMessage::Developer { content });
    } else {
        messages.push(ChatMessage::System { content });
    }
}

fn append_agent_message(
    messages: &mut Vec<ChatMessage>,
    author: String,
    recipient: String,
    content: Vec<AgentMessageInputContent>,
    capabilities: BackendCapabilities,
) -> Result<(), ProxyError> {
    if content
        .iter()
        .any(|item| matches!(item, AgentMessageInputContent::EncryptedContent { .. }))
    {
        return Err(ProxyError::unsupported("encrypted agent_message content"));
    }
    if let Some(text) = plaintext_agent_message_content(&content) {
        push_control_message(
            messages,
            format!("[Agent message from {author} to {recipient}]\n{text}"),
            capabilities,
        );
    }
    Ok(())
}

fn append_assistant_content(
    assistant: &mut PendingAssistant,
    content: Vec<ContentItem>,
) -> Result<(), ProxyError> {
    for item in content {
        match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                assistant.text.push(text);
            }
            ContentItem::InputImage { .. } => {
                return Err(ProxyError::unsupported(
                    "image content in an assistant message",
                ));
            }
            ContentItem::InputAudio { .. } => {
                return Err(ProxyError::unsupported(
                    "audio content in an assistant message",
                ));
            }
        }
    }
    Ok(())
}

fn flush_assistant(messages: &mut Vec<ChatMessage>, assistant: &mut PendingAssistant) {
    if assistant.reasoning_content.is_empty()
        && assistant.text.is_empty()
        && assistant.tool_calls.is_empty()
    {
        return;
    }
    messages.push(ChatMessage::Assistant {
        content: (!assistant.text.is_empty()).then(|| assistant.text.join("")),
        reasoning_content: (!assistant.reasoning_content.is_empty())
            .then(|| assistant.reasoning_content.join("")),
        tool_calls: std::mem::take(&mut assistant.tool_calls),
    });
    assistant.reasoning_content.clear();
    assistant.text.clear();
}

fn plaintext_reasoning(content: Option<Vec<ReasoningItemContent>>) -> Result<String, ProxyError> {
    let mut reasoning = String::new();
    for item in content.unwrap_or_default() {
        match item {
            ReasoningItemContent::ReasoningText { text } => reasoning.push_str(&text),
            ReasoningItemContent::Text { .. } => {
                return Err(ProxyError::unsupported(
                    "reasoning history with legacy text content",
                ));
            }
        }
    }
    Ok(reasoning)
}

fn user_content(
    content: Vec<ContentItem>,
    capabilities: BackendCapabilities,
) -> Result<Value, ProxyError> {
    let mut parts = Vec::with_capacity(content.len());
    let mut contains_image = false;
    for item in content {
        match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                parts.push(json!({"type": "text", "text": text}));
            }
            ContentItem::InputImage { image_url, detail } => {
                if !capabilities.image_input {
                    return Err(ProxyError::unsupported(
                        "image input; start the proxy with --supports-image-input or use text-only input",
                    ));
                }
                contains_image = true;
                let mut image = Map::from_iter([("url".to_string(), Value::String(image_url))]);
                if let Some(detail) = detail {
                    image.insert(
                        "detail".to_string(),
                        Value::String(image_detail_name(detail).to_string()),
                    );
                }
                parts.push(json!({"type": "image_url", "image_url": image}));
            }
            ContentItem::InputAudio { .. } => {
                return Err(ProxyError::unsupported("audio input"));
            }
        }
    }
    if !contains_image && parts.len() == 1 {
        return Ok(parts
            .pop()
            .and_then(|part| part.get("text").cloned())
            .unwrap_or_else(|| Value::String(String::new())));
    }
    Ok(Value::Array(parts))
}

fn text_content(content: Vec<ContentItem>, role: &str) -> Result<String, ProxyError> {
    let mut text = Vec::with_capacity(content.len());
    for item in content {
        match item {
            ContentItem::InputText { text: part } | ContentItem::OutputText { text: part } => {
                text.push(part);
            }
            ContentItem::InputImage { .. } => {
                return Err(ProxyError::unsupported(format!(
                    "image content in a {role} message"
                )));
            }
            ContentItem::InputAudio { .. } => {
                return Err(ProxyError::unsupported(format!(
                    "audio content in a {role} message"
                )));
            }
        }
    }
    Ok(text.join(""))
}

fn image_detail_name(detail: ImageDetail) -> &'static str {
    match detail {
        ImageDetail::Auto => "auto",
        ImageDetail::Low => "low",
        ImageDetail::High => "high",
        ImageDetail::Original => "original",
    }
}

fn chat_tool_call(call_id: String, name: String, arguments: String) -> ChatToolCall {
    ChatToolCall {
        id: call_id,
        kind: "function",
        function: ChatFunctionCall { name, arguments },
    }
}
