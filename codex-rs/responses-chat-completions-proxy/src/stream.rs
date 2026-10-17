use crate::config::ReasoningContentPolicy;
use crate::error::ProxyError;
use crate::protocol::BufferedChatCompletion;
use crate::protocol::ChatChunk;
use crate::protocol::ChatToolCallDelta;
use crate::protocol::ChatUsage;
use crate::tool_registry::ToolRegistry;
use async_stream::stream;
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures::Stream;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

mod reasoning;

use reasoning::ReasoningState;

pub(crate) enum UpstreamResponseBody {
    EventStream(reqwest::Response),
    Buffered(BufferedChatCompletion),
}

struct StreamTranslator {
    response_id: String,
    message_id: String,
    message_output_index: Option<usize>,
    next_output_index: usize,
    reasoning: ReasoningState,
    text: String,
    calls: BTreeMap<usize, PendingToolCall>,
    finish_reason: Option<String>,
    usage: Option<ChatUsage>,
}

struct PendingToolCall {
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
}

pub(crate) fn translate_upstream(
    body: UpstreamResponseBody,
    tools: ToolRegistry,
    idle_timeout: Duration,
    reasoning_content_policy: ReasoningContentPolicy,
) -> impl Stream<Item = Result<Bytes, Infallible>> + Send {
    stream! {
        let mut translator = StreamTranslator::new(reasoning_content_policy);
        yield Ok(sse(translator.created_event()));
        let result = match body {
            UpstreamResponseBody::EventStream(response) => {
                let mut events = response.bytes_stream().eventsource();
                loop {
                    let next = timeout(idle_timeout, events.next()).await;
                    let event = match next {
                        Ok(Some(Ok(event))) => event,
                        Ok(Some(Err(error))) => {
                            break Err(ProxyError::invalid_upstream(format!(
                                "failed to decode Chat SSE: {error}"
                            )));
                        }
                        Ok(None) => {
                            break if translator.finish_reason.is_some() {
                                translator.finish(&tools)
                            } else {
                                Err(ProxyError::invalid_upstream(
                                    "stream closed before a finish_reason or [DONE]",
                                ))
                            };
                        }
                        Err(_) => {
                            break Err(ProxyError::invalid_upstream(
                                "idle timeout waiting for Chat SSE",
                            ));
                        }
                    };
                    if event.data.trim() == "[DONE]" {
                        break translator.finish(&tools);
                    }
                    let chunk = match parse_chat_chunk(&event.data) {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            break Err(error);
                        }
                    };
                    match translator.push_chunk(chunk) {
                        Ok(events) => {
                            for event in events {
                                yield Ok(sse(event));
                            }
                        }
                        Err(error) => break Err(error),
                    }
                }
            }
            UpstreamResponseBody::Buffered(completion) => {
                match translator.push_buffered(completion) {
                    Ok(events) => {
                        for event in events {
                            yield Ok(sse(event));
                        }
                        translator.finish(&tools)
                    }
                    Err(error) => Err(error),
                }
            }
        };
        match result {
            Ok(events) => {
                for event in events {
                    yield Ok(sse(event));
                }
            }
            Err(error) => yield Ok(sse(translator.failed_event(error.to_string()))),
        }
    }
}

impl StreamTranslator {
    fn new(reasoning_content_policy: ReasoningContentPolicy) -> Self {
        Self {
            response_id: prefixed_id("resp"),
            message_id: prefixed_id("msg"),
            message_output_index: None,
            next_output_index: 0,
            reasoning: ReasoningState::new(reasoning_content_policy),
            text: String::new(),
            calls: BTreeMap::new(),
            finish_reason: None,
            usage: None,
        }
    }

    fn allocate_output_index(&mut self) -> usize {
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        output_index
    }

    fn start_message(&mut self, events: &mut Vec<Value>) -> usize {
        if let Some(output_index) = self.message_output_index {
            return output_index;
        }
        let output_index = self.allocate_output_index();
        self.message_output_index = Some(output_index);
        events.push(json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": {
                "id": self.message_id,
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "content": []
            }
        }));
        output_index
    }

    fn created_event(&self) -> Value {
        json!({
            "type": "response.created",
            "response": {
                "id": self.response_id,
                "object": "response",
                "status": "in_progress",
                "output": []
            }
        })
    }

    fn push_chunk(&mut self, chunk: ChatChunk) -> Result<Vec<Value>, ProxyError> {
        let _ = chunk.id;
        if let Some(usage) = chunk.usage {
            self.usage = Some(usage);
        }
        if chunk.choices.len() > 1 {
            return Err(ProxyError::invalid_upstream(
                "received more than one Chat completion choice",
            ));
        }
        let Some(choice) = chunk.choices.into_iter().next() else {
            return Ok(Vec::new());
        };
        if choice.index != 0 {
            return Err(ProxyError::invalid_upstream(format!(
                "received unsupported Chat choice index {}",
                choice.index
            )));
        }
        if let Some(refusal) = choice.delta.refusal
            && !refusal.is_empty()
        {
            return Err(ProxyError::unsupported(
                "Chat refusal content cannot be represented by the V1 Responses profile",
            ));
        }
        let mut events = Vec::new();
        if let Some(reasoning_content) = choice.delta.reasoning_content
            && !reasoning_content.is_empty()
        {
            events.extend(
                self.reasoning
                    .push_delta(reasoning_content, &mut self.next_output_index)?,
            );
        }
        if let Some(content) = choice.delta.content
            && !content.is_empty()
        {
            let output_index = self.start_message(&mut events);
            self.text.push_str(&content);
            events.push(json!({
                "type": "response.output_text.delta",
                "item_id": self.message_id,
                "output_index": output_index,
                "content_index": 0,
                "delta": content
            }));
        }
        for delta in choice.delta.tool_calls.unwrap_or_default() {
            self.push_tool_delta(delta)?;
        }
        if let Some(finish_reason) = choice.finish_reason {
            if let Some(existing) = &self.finish_reason
                && existing != &finish_reason
            {
                return Err(ProxyError::invalid_upstream(format!(
                    "conflicting finish reasons {existing:?} and {finish_reason:?}"
                )));
            }
            self.finish_reason = Some(finish_reason);
        }
        Ok(events)
    }

    fn push_buffered(
        &mut self,
        completion: BufferedChatCompletion,
    ) -> Result<Vec<Value>, ProxyError> {
        let _ = completion.id;
        self.usage = completion.usage;
        if completion.choices.len() != 1 {
            return Err(ProxyError::invalid_upstream(format!(
                "expected exactly one buffered Chat choice, received {}",
                completion.choices.len()
            )));
        }
        let choice =
            completion.choices.into_iter().next().ok_or_else(|| {
                ProxyError::invalid_upstream("buffered Chat response has no choice")
            })?;
        if choice.index != 0 {
            return Err(ProxyError::invalid_upstream(format!(
                "received unsupported buffered Chat choice index {}",
                choice.index
            )));
        }
        if choice
            .message
            .refusal
            .as_deref()
            .is_some_and(|refusal| !refusal.is_empty())
        {
            return Err(ProxyError::unsupported(
                "Chat refusal content cannot be represented by the V1 Responses profile",
            ));
        }
        let mut events = Vec::new();
        if choice.message.reasoning_content.is_some() || choice.message.content.is_some() {
            events.extend(self.push_chunk(ChatChunk {
                id: None,
                choices: vec![crate::protocol::ChatChoice {
                    index: 0,
                    delta: crate::protocol::ChatDelta {
                        content: choice.message.content,
                        reasoning_content: choice.message.reasoning_content,
                        refusal: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
            })?);
        }
        for (index, call) in choice.message.tool_calls.into_iter().enumerate() {
            self.push_tool_delta(ChatToolCallDelta {
                index,
                id: Some(call.id),
                function: Some(crate::protocol::ChatFunctionCallDelta {
                    name: Some(call.function.name),
                    arguments: Some(call.function.arguments),
                }),
            })?;
        }
        self.finish_reason = choice.finish_reason;
        Ok(events)
    }

    fn push_tool_delta(&mut self, delta: ChatToolCallDelta) -> Result<(), ProxyError> {
        let call = self
            .calls
            .entry(delta.index)
            .or_insert_with(|| PendingToolCall {
                item_id: prefixed_id("fc"),
                call_id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
        if let Some(call_id) = delta.id {
            if call.call_id.is_empty() {
                call.call_id = call_id;
            } else if call.call_id != call_id {
                return Err(ProxyError::invalid_upstream(format!(
                    "conflicting call IDs for tool-call index {}",
                    delta.index
                )));
            }
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                call.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                call.arguments.push_str(&arguments);
            }
        }
        Ok(())
    }

    fn finish(&mut self, tools: &ToolRegistry) -> Result<Vec<Value>, ProxyError> {
        let finish_reason = self.finish_reason.as_deref().ok_or_else(|| {
            ProxyError::invalid_upstream("Chat response ended without finish_reason")
        })?;
        match finish_reason {
            "length" => {
                return Ok(vec![json!({
                    "type": "response.incomplete",
                    "response": {
                        "id": self.response_id,
                        "status": "incomplete",
                        "incomplete_details": {"reason": "max_output_tokens"}
                    }
                })]);
            }
            "content_filter" => {
                return Ok(vec![self.failed_event(
                    "upstream Chat completion stopped because of content filtering".to_string(),
                )]);
            }
            "stop" if !self.calls.is_empty() => {
                return Err(ProxyError::invalid_upstream(
                    "finish_reason=stop accompanied by tool calls",
                ));
            }
            "tool_calls" if self.calls.is_empty() => {
                return Err(ProxyError::invalid_upstream(
                    "finish_reason=tool_calls without a tool call",
                ));
            }
            "stop" | "tool_calls" => {}
            unsupported => {
                return Err(ProxyError::invalid_upstream(format!(
                    "unsupported Chat finish_reason {unsupported:?}"
                )));
            }
        }

        let mut completed_items = Vec::new();
        if let Some(reasoning_events) = self.reasoning.finish_events() {
            completed_items.push(reasoning_events);
        }
        if let Some(output_index) = self.message_output_index {
            completed_items.push((
                output_index,
                vec![json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": {
                        "id": self.message_id,
                        "type": "message",
                        "status": "completed",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": self.text,
                            "annotations": []
                        }]
                    }
                })],
            ));
        }
        for call in self.calls.values_mut() {
            if call.name.is_empty() {
                return Err(ProxyError::invalid_upstream(
                    "tool call completed without a function name",
                ));
            }
            if call.call_id.is_empty() {
                call.call_id = prefixed_id("call");
            }
            let item = tools.response_item(
                call.item_id.clone(),
                call.call_id.clone(),
                &call.name,
                call.arguments.clone(),
            )?;
            let output_index = self.next_output_index;
            self.next_output_index += 1;
            completed_items.push((
                output_index,
                vec![json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": item
                })],
            ));
        }
        completed_items.sort_by_key(|(output_index, _)| *output_index);
        let mut events = completed_items
            .into_iter()
            .flat_map(|(_, events)| events)
            .collect::<Vec<_>>();
        let usage = self.usage.map(|usage| {
            let cached_tokens = usage
                .prompt_tokens_details
                .map(|details| details.cached_tokens)
                .unwrap_or(0);
            let reasoning_tokens = usage
                .completion_tokens_details
                .map(|details| details.reasoning_tokens)
                .unwrap_or(0);
            json!({
                "input_tokens": usage.prompt_tokens,
                "input_tokens_details": {"cached_tokens": cached_tokens},
                "output_tokens": usage.completion_tokens,
                "output_tokens_details": {"reasoning_tokens": reasoning_tokens},
                "total_tokens": usage.total_tokens
            })
        });
        events.push(json!({
            "type": "response.completed",
            "response": {
                "id": self.response_id,
                "object": "response",
                "status": "completed",
                "usage": usage
            }
        }));
        Ok(events)
    }

    fn failed_event(&self, message: String) -> Value {
        json!({
            "type": "response.failed",
            "response": {
                "id": self.response_id,
                "object": "response",
                "status": "failed",
                "error": {
                    "type": "server_error",
                    "code": "upstream_error",
                    "message": message
                }
            }
        })
    }
}

fn prefixed_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::now_v7().simple())
}

fn sse(value: Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

fn parse_chat_chunk(data: &str) -> Result<ChatChunk, ProxyError> {
    let value: Value = serde_json::from_str(data)
        .map_err(|error| ProxyError::invalid_upstream(format!("invalid Chat chunk: {error}")))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("upstream emitted an SSE error without a message");
        return Err(ProxyError::invalid_upstream(format!(
            "upstream SSE error: {message}"
        )));
    }
    serde_json::from_value(value)
        .map_err(|error| ProxyError::invalid_upstream(format!("invalid Chat chunk: {error}")))
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
