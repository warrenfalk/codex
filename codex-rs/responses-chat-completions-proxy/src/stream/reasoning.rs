use super::prefixed_id;
use crate::config::ReasoningContentPolicy;
use crate::error::ProxyError;
use serde_json::Value;
use serde_json::json;

pub(super) struct ReasoningState {
    policy: ReasoningContentPolicy,
    id: String,
    output_index: Option<usize>,
    text: String,
}

impl ReasoningState {
    pub(super) fn new(policy: ReasoningContentPolicy) -> Self {
        Self {
            policy,
            id: prefixed_id("rs"),
            output_index: None,
            text: String::new(),
        }
    }

    pub(super) fn push_delta(
        &mut self,
        delta: String,
        next_output_index: &mut usize,
    ) -> Result<Vec<Value>, ProxyError> {
        if self.policy != ReasoningContentPolicy::Plaintext {
            return Err(ProxyError::unsupported(
                "upstream plaintext reasoning_content; enable the backend reasoning-content policy",
            ));
        }

        let mut events = Vec::new();
        let output_index = match self.output_index {
            Some(output_index) => output_index,
            None => {
                let output_index = *next_output_index;
                *next_output_index += 1;
                self.output_index = Some(output_index);
                events.push(json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": {
                        "id": self.id,
                        "type": "reasoning",
                        "status": "in_progress",
                        "summary": []
                    }
                }));
                output_index
            }
        };
        self.text.push_str(&delta);
        events.push(json!({
            "type": "response.reasoning_text.delta",
            "item_id": self.id,
            "output_index": output_index,
            "content_index": 0,
            "delta": delta
        }));
        Ok(events)
    }

    pub(super) fn finish_events(&self) -> Option<(usize, Vec<Value>)> {
        let output_index = self.output_index?;
        Some((
            output_index,
            vec![
                json!({
                    "type": "response.reasoning_text.done",
                    "item_id": self.id,
                    "output_index": output_index,
                    "content_index": 0,
                    "text": self.text
                }),
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": {
                        "id": self.id,
                        "type": "reasoning",
                        "status": "completed",
                        "summary": [],
                        "content": [{
                            "type": "reasoning_text",
                            "text": self.text
                        }],
                        "encrypted_content": null
                    }
                }),
            ],
        ))
    }
}
