use super::*;
use crate::config::ReasoningContentPolicy;
use crate::protocol::ChatChoice;
use crate::protocol::ChatDelta;
use crate::protocol::ChatFunctionCallDelta;
use crate::tool_registry::ToolIdentity;
use pretty_assertions::assert_eq;
use serde_json::json;

fn text_chunk(content: &str, finish_reason: Option<&str>, usage: Option<ChatUsage>) -> ChatChunk {
    ChatChunk {
        id: Some("chatcmpl_1".to_string()),
        choices: vec![ChatChoice {
            index: 0,
            delta: ChatDelta {
                content: Some(content.to_string()),
                reasoning_content: None,
                refusal: None,
                tool_calls: None,
            },
            finish_reason: finish_reason.map(str::to_string),
        }],
        usage,
    }
}

#[test]
fn streams_text_then_emits_a_complete_item_and_terminal_event() -> anyhow::Result<()> {
    let mut translator = StreamTranslator::new(ReasoningContentPolicy::Unsupported);
    let first = translator.push_chunk(text_chunk("hel", None, None))?;
    let second = translator.push_chunk(text_chunk(
        "lo",
        Some("stop"),
        Some(ChatUsage {
            prompt_tokens: 3,
            completion_tokens: 2,
            total_tokens: 5,
            prompt_tokens_details: Some(crate::protocol::ChatPromptTokenDetails {
                cached_tokens: 1,
            }),
            completion_tokens_details: Some(crate::protocol::ChatCompletionTokenDetails {
                reasoning_tokens: 1,
            }),
        }),
    ))?;
    let terminal = translator.finish(&ToolRegistry::default())?;

    assert_eq!(first[0]["type"], json!("response.output_item.added"));
    assert_eq!(first[1]["delta"], json!("hel"));
    assert_eq!(second[0]["delta"], json!("lo"));
    assert_eq!(terminal[0]["type"], json!("response.output_item.done"));
    assert_eq!(terminal[0]["item"]["content"][0]["text"], json!("hello"));
    assert_eq!(terminal[1]["type"], json!("response.completed"));
    assert_eq!(
        terminal[1]["response"]["usage"],
        json!({
            "input_tokens": 3,
            "input_tokens_details": {"cached_tokens": 1},
            "output_tokens": 2,
            "output_tokens_details": {"reasoning_tokens": 1},
            "total_tokens": 5
        })
    );
    Ok(())
}

#[test]
fn captures_plaintext_reasoning_before_assistant_text() -> anyhow::Result<()> {
    let mut translator = StreamTranslator::new(ReasoningContentPolicy::Plaintext);
    let reasoning = translator.push_chunk(ChatChunk {
        id: Some("chatcmpl_1".to_string()),
        choices: vec![ChatChoice {
            index: 0,
            delta: ChatDelta {
                content: None,
                reasoning_content: Some("Inspect carefully.".to_string()),
                refusal: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    })?;
    let text = translator.push_chunk(text_chunk("Done.", Some("stop"), None))?;

    let terminal = translator.finish(&ToolRegistry::default())?;

    assert_eq!(reasoning[0]["type"], json!("response.output_item.added"));
    assert_eq!(reasoning[0]["output_index"], json!(0));
    assert_eq!(reasoning[0]["item"]["type"], json!("reasoning"));
    assert_eq!(reasoning[1]["type"], json!("response.reasoning_text.delta"));
    assert_eq!(reasoning[1]["delta"], json!("Inspect carefully."));
    assert_eq!(text[0]["output_index"], json!(1));
    assert_eq!(terminal[0]["type"], json!("response.reasoning_text.done"));
    assert_eq!(terminal[1]["type"], json!("response.output_item.done"));
    assert_eq!(terminal[1]["output_index"], json!(0));
    assert_eq!(
        terminal[1]["item"]["content"],
        json!([{"type": "reasoning_text", "text": "Inspect carefully."}])
    );
    assert_eq!(terminal[2]["item"]["type"], json!("message"));
    assert_eq!(terminal[2]["output_index"], json!(1));
    assert_eq!(terminal[3]["type"], json!("response.completed"));
    Ok(())
}

#[test]
fn rejects_unexpected_plaintext_reasoning() {
    let mut translator = StreamTranslator::new(ReasoningContentPolicy::Unsupported);

    let error = translator
        .push_chunk(ChatChunk {
            id: None,
            choices: vec![ChatChoice {
                index: 0,
                delta: ChatDelta {
                    content: None,
                    reasoning_content: Some("not declared".to_string()),
                    refusal: None,
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        })
        .err()
        .map(|error| error.to_string());

    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("reasoning-content policy"))
    );
}

#[test]
fn restores_parallel_function_and_custom_calls() -> anyhow::Result<()> {
    let registry = ToolRegistry::from_responses_tools(Some(vec![
        json!({
            "type": "function",
            "name": "shell",
            "description": "Run",
            "strict": false,
            "parameters": {"type": "object"}
        }),
        json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Patch",
            "format": {"type": "text", "syntax": "patch", "definition": ""}
        }),
    ]))?;
    let custom_name = registry.safe_name(&ToolIdentity::custom("apply_patch", None));
    let mut translator = StreamTranslator::new(ReasoningContentPolicy::Unsupported);
    translator.push_chunk(ChatChunk {
        id: None,
        choices: vec![ChatChoice {
            index: 0,
            delta: ChatDelta {
                content: Some("working".to_string()),
                reasoning_content: None,
                refusal: None,
                tool_calls: Some(vec![
                    ChatToolCallDelta {
                        index: 1,
                        id: Some("call_patch".to_string()),
                        function: Some(ChatFunctionCallDelta {
                            name: Some(custom_name),
                            arguments: Some("{\"input\":\"*** Begin ".to_string()),
                        }),
                    },
                    ChatToolCallDelta {
                        index: 0,
                        id: Some("call_shell".to_string()),
                        function: Some(ChatFunctionCallDelta {
                            name: Some("shell".to_string()),
                            arguments: Some("{\"cmd\":\"pwd\"}".to_string()),
                        }),
                    },
                ]),
            },
            finish_reason: None,
        }],
        usage: None,
    })?;
    translator.push_chunk(ChatChunk {
        id: None,
        choices: vec![ChatChoice {
            index: 0,
            delta: ChatDelta {
                content: None,
                reasoning_content: None,
                refusal: None,
                tool_calls: Some(vec![ChatToolCallDelta {
                    index: 1,
                    id: None,
                    function: Some(ChatFunctionCallDelta {
                        name: None,
                        arguments: Some("Patch\"}".to_string()),
                    }),
                }]),
            },
            finish_reason: Some("tool_calls".to_string()),
        }],
        usage: None,
    })?;

    let events = translator.finish(&registry)?;

    assert_eq!(events[0]["item"]["type"], json!("message"));
    assert_eq!(events[0]["item"]["content"][0]["text"], json!("working"));
    assert_eq!(events[1]["item"]["type"], json!("function_call"));
    assert_eq!(events[1]["item"]["call_id"], json!("call_shell"));
    assert_eq!(events[2]["item"]["type"], json!("custom_tool_call"));
    assert_eq!(events[2]["item"]["input"], json!("*** Begin Patch"));
    assert_eq!(events[3]["type"], json!("response.completed"));
    Ok(())
}

#[test]
fn maps_length_finish_to_incomplete_without_completed() -> anyhow::Result<()> {
    let mut translator = StreamTranslator::new(ReasoningContentPolicy::Unsupported);
    translator.push_chunk(text_chunk("partial", Some("length"), None))?;

    let events = translator.finish(&ToolRegistry::default())?;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], json!("response.incomplete"));
    assert_eq!(
        events[0]["response"]["incomplete_details"]["reason"],
        json!("max_output_tokens")
    );
    Ok(())
}

#[test]
fn maps_content_filter_to_failed_and_rejects_unknown_terminal_reasons() -> anyhow::Result<()> {
    let mut filtered = StreamTranslator::new(ReasoningContentPolicy::Unsupported);
    filtered.push_chunk(text_chunk("partial", Some("content_filter"), None))?;

    let filtered_events = filtered.finish(&ToolRegistry::default())?;

    assert_eq!(filtered_events.len(), 1);
    assert_eq!(filtered_events[0]["type"], json!("response.failed"));

    let mut unknown = StreamTranslator::new(ReasoningContentPolicy::Unsupported);
    unknown.push_chunk(text_chunk("partial", Some("future_reason"), None))?;
    let error = unknown
        .finish(&ToolRegistry::default())
        .err()
        .map(|error| error.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "upstream Chat Completions stream is invalid: unsupported Chat finish_reason \"future_reason\""
        )
    );
    Ok(())
}
