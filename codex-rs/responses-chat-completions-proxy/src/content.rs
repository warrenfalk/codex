use crate::config::BackendCapabilities;
use crate::error::ProxyError;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageDetail;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

pub(crate) fn tool_content(
    output: FunctionCallOutputBody,
    capabilities: BackendCapabilities,
) -> Result<Value, ProxyError> {
    let items = match output {
        FunctionCallOutputBody::Text(text) => return Ok(Value::String(text)),
        FunctionCallOutputBody::ContentItems(items) => items,
    };
    let mut content = Vec::with_capacity(items.len());
    let mut contains_image = false;
    for item in items {
        content.push(match item {
            FunctionCallOutputContentItem::InputText { text } => ContentItem::InputText { text },
            FunctionCallOutputContentItem::InputImage { image_url, detail } => {
                if !capabilities.image_tool_output {
                    return Err(ProxyError::unsupported(
                        "image tool-call output; start the proxy with --supports-image-input and --supports-image-tool-output or use a profile that enables them",
                    ));
                }
                contains_image = true;
                ContentItem::InputImage { image_url, detail }
            }
            FunctionCallOutputContentItem::InputAudio { .. } => {
                return Err(ProxyError::unsupported("audio tool-call output"));
            }
            FunctionCallOutputContentItem::EncryptedContent { .. } => {
                return Err(ProxyError::unsupported("encrypted tool-call output"));
            }
        });
    }
    if contains_image {
        user_content(content, capabilities)
    } else {
        // Text-only Chat tool results remain strings, including structured MCP results.
        let text = content
            .into_iter()
            .filter_map(|item| match item {
                ContentItem::InputText { text } | ContentItem::OutputText { text } => Some(text),
                ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Value::String(text))
    }
}

pub(crate) fn user_content(
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

fn image_detail_name(detail: ImageDetail) -> &'static str {
    match detail {
        ImageDetail::Auto => "auto",
        ImageDetail::Low => "low",
        ImageDetail::High => "high",
        ImageDetail::Original => "original",
    }
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
