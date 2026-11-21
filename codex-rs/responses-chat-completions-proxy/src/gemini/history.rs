use super::MAX_REPLAY_BYTES;
use super::REPLAY_PREFIX;
use super::ReplayPart;
use crate::error::ProxyError;
use crate::tool_registry::ToolIdentity;
use crate::tool_registry::ToolKind;
use crate::tool_registry::ToolRegistry;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseItem;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct History {
    pub(super) instructions: Vec<Value>,
    pub(super) contents: Vec<Value>,
}

struct Call {
    id: String,
    name: String,
    args: Value,
    kind: ToolKind,
    omit_provider_id: bool,
}

#[derive(Default)]
struct Batch {
    text: String,
    replay: Option<Vec<ReplayPart>>,
    calls: Vec<Call>,
    outputs: HashMap<String, Value>,
}

pub(super) fn compile(
    instructions: String,
    input: Vec<Value>,
    tools: &ToolRegistry,
) -> Result<History, ProxyError> {
    let mut history = History::default();
    if !instructions.is_empty() {
        history.instructions.push(json!({"text": instructions}));
    }
    let mut batch = Batch::default();
    let mut call_ids = HashSet::new();
    for value in input {
        let item: ResponseItem = serde_json::from_value(value)
            .map_err(|_| ProxyError::invalid("invalid Gemini history item"))?;
        if !batch.outputs.is_empty()
            && !matches!(
                item,
                ResponseItem::FunctionCallOutput { .. } | ResponseItem::CustomToolCallOutput { .. }
            )
        {
            batch.flush(&mut history)?;
        }
        match item {
            ResponseItem::Message { role, content, .. } => match role.as_str() {
                "user" => {
                    batch.flush(&mut history)?;
                    let parts = content
                        .into_iter()
                        .map(|part| match part {
                            ContentItem::InputText { text }
                            | ContentItem::OutputText { text, .. } => Ok(json!({"text": text})),
                            ContentItem::InputImage { image, .. } => {
                                Ok(json!({"inlineData": image_blob(image)?}))
                            }
                            ContentItem::InputAudio { .. } => {
                                Err(ProxyError::unsupported("Gemini audio input"))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if !parts.is_empty() {
                        history
                            .contents
                            .push(json!({"role": "user", "parts": parts}));
                    }
                }
                "assistant" | "system" | "developer" => {
                    for part in content {
                        let (ContentItem::InputText { text }
                        | ContentItem::OutputText { text, .. }) = part
                        else {
                            return Err(ProxyError::unsupported(
                                "non-text Gemini instruction or assistant history",
                            ));
                        };
                        if role == "assistant" {
                            batch.text.push_str(&text);
                        } else {
                            history.instructions.push(json!({"text": text}));
                        }
                    }
                }
                _ => {
                    return Err(ProxyError::unsupported(format!(
                        "Gemini message role {role:?}"
                    )));
                }
            },
            ResponseItem::Reasoning {
                encrypted_content, ..
            } => {
                if let Some(replay) = encrypted_content
                    .as_deref()
                    .and_then(|data| data.strip_prefix(REPLAY_PREFIX))
                {
                    if replay.len() > MAX_REPLAY_BYTES || batch.replay.is_some() {
                        return Err(ProxyError::invalid(
                            "oversized or duplicate Gemini replay metadata",
                        ));
                    }
                    batch.replay =
                        Some(serde_json::from_str(replay).map_err(|_| {
                            ProxyError::invalid("malformed Gemini replay metadata")
                        })?);
                }
            }
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                call_id,
                ..
            } => {
                let args: Value = serde_json::from_str(&arguments).map_err(|_| {
                    ProxyError::invalid("invalid Gemini function arguments in history")
                })?;
                batch.push_call(
                    &mut call_ids,
                    Call {
                        id: call_id,
                        name: tools.safe_name(&ToolIdentity::function(&name, namespace.as_deref())),
                        args,
                        kind: ToolKind::Function,
                        omit_provider_id: false,
                    },
                )?;
            }
            ResponseItem::CustomToolCall {
                name,
                namespace,
                input,
                call_id,
                ..
            } => {
                batch.push_call(
                    &mut call_ids,
                    Call {
                        id: call_id,
                        name: tools.safe_name(&ToolIdentity::custom(&name, namespace.as_deref())),
                        args: json!({"input": input}),
                        kind: ToolKind::Custom,
                        omit_provider_id: false,
                    },
                )?;
            }
            ResponseItem::FunctionCallOutput {
                call_id, output, ..
            } => {
                let id = call_id
                    .ok_or_else(|| ProxyError::invalid("Gemini function output has no call_id"))?;
                batch.output(id, ToolKind::Function, output.body, output.success)?;
            }
            ResponseItem::CustomToolCallOutput {
                call_id, output, ..
            } => {
                batch.output(call_id, ToolKind::Custom, output.body, output.success)?;
            }
            _ => return Err(ProxyError::unsupported("Gemini history item type")),
        }
    }
    batch.flush(&mut history)?;
    Ok(history)
}

impl Batch {
    fn push_call(&mut self, call_ids: &mut HashSet<String>, call: Call) -> Result<(), ProxyError> {
        if call.id.is_empty() || !call.args.is_object() {
            return Err(ProxyError::invalid(
                "Gemini call requires an ID and object arguments",
            ));
        }
        if !call_ids.insert(call.id.clone()) {
            return Err(ProxyError::invalid("duplicate Gemini history call ID"));
        }
        self.calls.push(call);
        Ok(())
    }

    fn output(
        &mut self,
        id: String,
        kind: ToolKind,
        body: FunctionCallOutputBody,
        success: Option<bool>,
    ) -> Result<(), ProxyError> {
        if !self
            .calls
            .iter()
            .any(|call| call.id == id && call.kind == kind)
            || self.outputs.contains_key(&id)
        {
            return Err(ProxyError::invalid(
                "orphaned, mismatched or duplicate Gemini tool output",
            ));
        }
        self.outputs.insert(id, tool_result(body, success)?);
        Ok(())
    }

    fn flush(&mut self, history: &mut History) -> Result<(), ProxyError> {
        if self.calls.len() != self.outputs.len() {
            return Err(ProxyError::invalid(
                "Gemini tool calls have unresolved or duplicate IDs",
            ));
        }
        let mut parts = Vec::new();
        let mut text = self.text.as_str();
        let mut calls = self.calls.iter_mut();
        if let Some(replay) = self.replay.take() {
            for part in replay {
                let (mut value, signature) =
                    match part {
                        ReplayPart::Text { bytes, signature } => {
                            let (head, tail) = text.split_at_checked(bytes).ok_or_else(|| {
                                ProxyError::invalid(
                                    "Gemini replay text does not match assistant history",
                                )
                            })?;
                            text = tail;
                            (json!({"text": head}), signature)
                        }
                        ReplayPart::Function {
                            call_id,
                            signature,
                            omit_provider_id,
                        } => {
                            let call = calls.next().filter(|call| call.id == call_id).ok_or_else(
                                || {
                                    ProxyError::invalid(
                                        "Gemini replay call does not match tool history",
                                    )
                                },
                            )?;
                            call.omit_provider_id = omit_provider_id;
                            (call.part(), signature)
                        }
                    };
                if let Some(signature) = signature {
                    value["thoughtSignature"] = json!(signature);
                }
                parts.push(value);
            }
            if !text.is_empty() || calls.next().is_some() {
                return Err(ProxyError::invalid(
                    "Gemini replay does not cover the full model response",
                ));
            }
        } else {
            if !text.is_empty() {
                parts.push(json!({"text": text}));
            }
            parts.extend(calls.map(|call| call.part()));
        }
        if !parts.is_empty() {
            history
                .contents
                .push(json!({"role": "model", "parts": parts}));
        }
        if !self.calls.is_empty() {
            let parts = self
                .calls
                .drain(..)
                .map(|call| {
                    let mut result = self
                        .outputs
                        .remove(&call.id)
                        .ok_or_else(|| ProxyError::invalid("missing Gemini tool output"))?;
                    result["name"] = json!(call.name);
                    if !call.omit_provider_id {
                        result["id"] = json!(call.id);
                    }
                    Ok(json!({"functionResponse": result}))
                })
                .collect::<Result<Vec<_>, ProxyError>>()?;
            history
                .contents
                .push(json!({"role": "user", "parts": parts}));
        }
        self.text.clear();
        Ok(())
    }
}

impl Call {
    fn part(&self) -> Value {
        let mut call = json!({"name": self.name, "args": self.args});
        if !self.omit_provider_id {
            call["id"] = json!(self.id);
        }
        json!({"functionCall": call})
    }
}

fn tool_result(body: FunctionCallOutputBody, success: Option<bool>) -> Result<Value, ProxyError> {
    let field = if success == Some(false) {
        "error"
    } else {
        "output"
    };
    let items = match body {
        FunctionCallOutputBody::Text(text) => return Ok(json!({"response": {(field): text}})),
        FunctionCallOutputBody::ContentItems(items) => items,
    };
    let mut output = Vec::with_capacity(items.len());
    let mut parts = Vec::new();
    for item in items {
        match item {
            FunctionCallOutputContentItem::InputText { text } => output.push(json!(text)),
            FunctionCallOutputContentItem::InputImage { image, .. } => {
                let mut blob = image_blob(image)?;
                let name = format!("image-{}", parts.len());
                blob["displayName"] = json!(name);
                parts.push(json!({"inlineData": blob}));
                output.push(json!({"$ref": name}));
            }
            FunctionCallOutputContentItem::InputAudio { .. }
            | FunctionCallOutputContentItem::EncryptedContent { .. } => {
                return Err(ProxyError::unsupported(
                    "Gemini audio or encrypted tool output",
                ));
            }
        }
    }
    let mut result = json!({"response": {(field): output}});
    if !parts.is_empty() {
        result["parts"] = json!(parts);
    }
    Ok(result)
}

fn image_blob(image: ImageReference) -> Result<Value, ProxyError> {
    let ImageReference::Inline { image_url } = image else {
        return Err(ProxyError::unsupported(
            "Gemini image input requires an inline data URL",
        ));
    };
    let (mime_type, data) = image_url
        .strip_prefix("data:")
        .and_then(|url| url.split_once(";base64,"))
        .filter(|(mime_type, data)| mime_type.starts_with("image/") && !data.is_empty())
        .ok_or_else(|| {
            ProxyError::unsupported("Gemini image input requires a base64 image data URL")
        })?;
    Ok(json!({"mimeType": mime_type, "data": data}))
}
