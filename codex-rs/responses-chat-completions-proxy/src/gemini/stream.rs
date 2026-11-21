use super::MAX_OUTPUT_BYTES;
use super::MAX_REPLAY_BYTES;
use super::MAX_STREAM_EVENT_BYTES;
use super::REPLAY_PREFIX;
use super::ReplayPart;
use crate::error::ProxyError;
use crate::stream::prefixed_id;
use crate::stream::sse;
use crate::tool_registry::ToolRegistry;
use async_stream::stream;
use bytes::Bytes;
use eventsource_stream::EventStreamError;
use eventsource_stream::Eventsource;
use futures::Stream;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::collections::HashSet;
use std::convert::Infallible;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Chunk {
    #[serde(default)]
    candidates: Vec<Candidate>,
    prompt_feedback: Option<PromptFeedback>,
    usage_metadata: Option<Usage>,
    error: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptFeedback {
    block_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    #[serde(default)]
    index: usize,
    content: Option<Content>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Content {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Part {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
    thought_signature: Option<String>,
    function_call: Option<FunctionCall>,
}

#[derive(Deserialize)]
struct FunctionCall {
    id: Option<String>,
    name: String,
    #[serde(default = "empty_args")]
    args: Value,
}

fn empty_args() -> Value {
    json!({})
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Usage {
    prompt_token_count: u64,
    cached_content_token_count: u64,
    candidates_token_count: u64,
    thoughts_token_count: u64,
    total_token_count: Option<u64>,
}

impl Usage {
    fn into_responses(self) -> Value {
        let output = self
            .candidates_token_count
            .saturating_add(self.thoughts_token_count);
        json!({
            "input_tokens": self.prompt_token_count,
            "input_tokens_details": {"cached_tokens": self.cached_content_token_count},
            "output_tokens": output,
            "output_tokens_details": {"reasoning_tokens": self.thoughts_token_count},
            "total_tokens": self.total_token_count.unwrap_or_else(|| self.prompt_token_count.saturating_add(output)),
        })
    }
}

struct Translation {
    response_id: String,
    message_id: String,
    text: String,
    layout: Vec<ReplayPart>,
    calls: Vec<Value>,
    call_ids: HashSet<String>,
    usage: Option<Usage>,
    finish_reason: Option<String>,
}

pub(super) fn translate(
    response: reqwest::Response,
    tools: ToolRegistry,
    idle_timeout: Duration,
) -> impl Stream<Item = Result<Bytes, Infallible>> + Send {
    stream! {
        let mut state = Translation {
            response_id: prefixed_id("resp"),
            message_id: prefixed_id("msg"),
            text: String::new(),
            layout: Vec::new(),
            calls: Vec::new(),
            call_ids: HashSet::new(),
            usage: None,
            finish_reason: None,
        };
        yield Ok(sse(json!({
            "type": "response.created",
            "response": {"id": state.response_id, "object": "response", "status": "in_progress", "output": []}
        })));
        let mut total_bytes = 0usize;
        let mut events = response.bytes_stream().map(move |chunk| {
            let chunk = chunk.map_err(|_| ProxyError::upstream("Gemini SSE transport failed"))?;
            total_bytes = total_bytes.saturating_add(chunk.len());
            if total_bytes > MAX_OUTPUT_BYTES {
                return Err(ProxyError::invalid_upstream("Gemini response exceeds the stream size limit"));
            }
            Ok(chunk)
        }).eventsource();
        let result = loop {
            let event = match timeout(idle_timeout, events.next()).await {
                Ok(Some(Ok(event))) if event.data == "[DONE]" => break state.finish(),
                Ok(Some(Ok(event))) => event,
                Ok(None) => break state.finish(),
                Ok(Some(Err(EventStreamError::Transport(error)))) => break Err(error),
                Ok(Some(Err(_))) => break Err(ProxyError::invalid_upstream("failed to decode Gemini SSE")),
                Err(_) => break Err(ProxyError::upstream("idle timeout waiting for Gemini SSE")),
            };
            if event.data.len() > MAX_STREAM_EVENT_BYTES {
                break Err(ProxyError::invalid_upstream("Gemini SSE event exceeds the size limit"));
            }
            let chunk = match serde_json::from_str::<Chunk>(&event.data) {
                Ok(chunk) => chunk,
                Err(_) => break Err(ProxyError::invalid_upstream("invalid Gemini stream chunk")),
            };
            match state.record(chunk, &tools) {
                Ok(deltas) => {
                    for delta in deltas {
                        yield Ok(sse(delta));
                    }
                }
                Err(error) => break Err(error),
            }
        };
        match result {
            Ok(events) => {
                for event in events {
                    yield Ok(sse(event));
                }
            }
            Err(error) => yield Ok(sse(json!({
                "type": "response.failed",
                "response": {
                    "id": state.response_id, "status": "failed",
                    "error": {"type": "server_error", "code": "upstream_error", "message": error.to_string()}
                }
            }))),
        }
    }
}

impl Translation {
    fn record(&mut self, chunk: Chunk, tools: &ToolRegistry) -> Result<Vec<Value>, ProxyError> {
        if chunk.error.is_some() {
            return Err(ProxyError::upstream(
                "Gemini reported an error after streaming began",
            ));
        }
        if let Some(reason) = chunk
            .prompt_feedback
            .and_then(|feedback| feedback.block_reason)
            && reason != "BLOCK_REASON_UNSPECIFIED"
        {
            return Err(ProxyError::upstream("Gemini blocked the prompt"));
        }
        if let Some(usage) = chunk.usage_metadata {
            self.usage = Some(usage);
        }
        if chunk.candidates.len() > 1 {
            return Err(ProxyError::invalid_upstream(
                "Gemini returned multiple candidates",
            ));
        }
        let Some(candidate) = chunk.candidates.into_iter().next() else {
            return Ok(Vec::new());
        };
        if candidate.index != 0 {
            return Err(ProxyError::invalid_upstream(
                "Gemini returned a nonzero candidate index",
            ));
        }
        if let Some(reason) = candidate.finish_reason
            && reason != "FINISH_REASON_UNSPECIFIED"
            && let Some(existing) = self.finish_reason.replace(reason.clone())
            && existing != reason
        {
            return Err(ProxyError::invalid_upstream(
                "Gemini returned conflicting finish reasons",
            ));
        }
        let mut events = Vec::new();
        for part in candidate
            .content
            .into_iter()
            .flat_map(|content| content.parts)
        {
            self.record_part(part, tools, &mut events)?;
        }
        Ok(events)
    }

    fn record_part(
        &mut self,
        part: Part,
        tools: &ToolRegistry,
        events: &mut Vec<Value>,
    ) -> Result<(), ProxyError> {
        let signature = part.thought_signature;
        if signature
            .as_ref()
            .is_some_and(|signature| signature.is_empty() || signature.len() > MAX_REPLAY_BYTES)
            || self.layout.len() >= MAX_REPLAY_BYTES / 32
        {
            return Err(ProxyError::invalid_upstream(
                "invalid or oversized Gemini replay metadata",
            ));
        }
        match (part.text, part.function_call, part.thought) {
            (Some(text), None, false) => {
                if !text.is_empty() {
                    if self.text.is_empty() {
                        events.push(json!({
                            "type": "response.output_item.added", "output_index": 0,
                            "item": {"id": self.message_id, "type": "message", "status": "in_progress", "role": "assistant", "content": []},
                        }));
                    }
                    self.text.push_str(&text);
                    events.push(json!({
                        "type": "response.output_text.delta", "item_id": self.message_id,
                        "output_index": 0, "content_index": 0, "delta": text,
                    }));
                }
                if signature.is_none()
                    && let Some(ReplayPart::Text {
                        bytes,
                        signature: None,
                    }) = self.layout.last_mut()
                {
                    *bytes += text.len();
                } else if signature.is_some() || !text.is_empty() {
                    self.layout.push(ReplayPart::Text {
                        bytes: text.len(),
                        signature,
                    });
                }
            }
            (None, Some(call), false) if call.args.is_object() => {
                let omit_provider_id = call.id.is_none();
                let call_id = call.id.unwrap_or_else(|| prefixed_id("call"));
                if call_id.is_empty() || !self.call_ids.insert(call_id.clone()) {
                    return Err(ProxyError::invalid_upstream(
                        "Gemini returned an empty or duplicate function call ID",
                    ));
                }
                let item = tools.response_item(
                    prefixed_id("fc"),
                    call_id.clone(),
                    &call.name,
                    call.args.to_string(),
                )?;
                if item["type"] == "custom_tool_call"
                    && call.args.as_object().is_some_and(|args| args.len() != 1)
                {
                    return Err(ProxyError::invalid_upstream(
                        "Gemini custom tool arguments must contain only input",
                    ));
                }
                self.calls.push(item);
                self.layout.push(ReplayPart::Function {
                    call_id,
                    signature,
                    omit_provider_id,
                });
            }
            (Some(_), None, true) if signature.is_none() => {}
            _ => {
                return Err(ProxyError::invalid_upstream(
                    "unsupported Gemini output part",
                ));
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<Vec<Value>, ProxyError> {
        match self.finish_reason.as_deref() {
            Some("STOP") => {}
            Some("MAX_TOKENS") => {
                return Ok(vec![json!({
                    "type": "response.incomplete",
                    "response": {"id": self.response_id, "status": "incomplete",
                        "incomplete_details": {"reason": "max_output_tokens"}}
                })]);
            }
            Some(_) => {
                return Err(ProxyError::upstream(
                    "Gemini ended with a failure finish reason",
                ));
            }
            None => {
                return Err(ProxyError::invalid_upstream(
                    "Gemini stream closed without a finish reason",
                ));
            }
        }
        if let Some(ReplayPart::Function {
            signature: None, ..
        }) = self
            .layout
            .iter()
            .find(|part| matches!(part, ReplayPart::Function { .. }))
        {
            return Err(ProxyError::invalid_upstream(
                "first Gemini function call has no thought signature",
            ));
        }
        let mut items = Vec::new();
        if !self.text.is_empty() {
            items.push(json!({
                "id": self.message_id, "type": "message", "status": "completed", "role": "assistant",
                "content": [{"type": "output_text", "text": self.text, "annotations": []}]
            }));
        }
        if !matches!(
            self.layout.as_slice(),
            [] | [ReplayPart::Text {
                signature: None,
                ..
            }]
        ) {
            let replay = serde_json::to_string(&self.layout).map_err(|_| {
                ProxyError::invalid_upstream("cannot serialize Gemini replay metadata")
            })?;
            if replay.len() > MAX_REPLAY_BYTES {
                return Err(ProxyError::invalid_upstream(
                    "Gemini replay metadata exceeds its size limit",
                ));
            }
            items.push(json!({
                "id": prefixed_id("rs"), "type": "reasoning", "summary": [],
                "encrypted_content": format!("{REPLAY_PREFIX}{replay}"),
            }));
        }
        items.append(&mut self.calls);
        let mut events = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                json!({
                    "type": "response.output_item.done", "output_index": index, "item": item,
                })
            })
            .collect::<Vec<_>>();
        events.push(json!({
            "type": "response.completed",
            "response": {
                "id": self.response_id, "object": "response", "status": "completed",
                "usage": self.usage.take().map(Usage::into_responses),
            }
        }));
        Ok(events)
    }
}
