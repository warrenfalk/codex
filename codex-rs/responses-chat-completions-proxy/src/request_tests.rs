use super::*;
use crate::PromptCacheKeyPolicy;
use crate::ReasoningContentPolicy;
use crate::protocol::ChatFunctionCall;
use crate::protocol::ChatMessage;
use crate::protocol::ChatToolCall;
use pretty_assertions::assert_eq;
use serde_json::json;

fn parse_request(input: Value) -> anyhow::Result<ResponsesRequest> {
    Ok(serde_json::from_value(input)?)
}

fn capabilities() -> BackendCapabilities {
    BackendCapabilities {
        parallel_tool_calls: true,
        ..BackendCapabilities::default()
    }
}

#[test]
fn compiles_calls_outputs_and_text_into_chat_history() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "codex-test",
        "instructions": "Be exact.",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "Inspect it"}]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "I will inspect."}]
            },
            {
                "type": "function_call",
                "name": "shell",
                "arguments": "{\"cmd\":\"pwd\"}",
                "call_id": "call_1"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "/workspace"
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Done."}]
            }
        ],
        "tools": [{
            "type": "function",
            "name": "shell",
            "description": "Run a command",
            "strict": false,
            "parameters": {"type": "object"}
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "stream": true
    }))?;

    let translated = translate_request(request, capabilities(), None)?;

    assert_eq!(
        translated.chat.messages,
        vec![
            ChatMessage::System {
                content: "Be exact.".to_string()
            },
            ChatMessage::User {
                content: json!("Inspect it")
            },
            ChatMessage::Assistant {
                content: Some("I will inspect.".to_string()),
                reasoning_content: None,
                tool_calls: vec![ChatToolCall {
                    id: "call_1".to_string(),
                    kind: "function",
                    function: ChatFunctionCall {
                        name: "shell".to_string(),
                        arguments: "{\"cmd\":\"pwd\"}".to_string()
                    }
                }]
            },
            ChatMessage::Tool {
                tool_call_id: "call_1".to_string(),
                content: "/workspace".to_string()
            },
            ChatMessage::Assistant {
                content: Some("Done.".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new()
            }
        ]
    );
    assert_eq!(translated.chat.model, "codex-test");
    assert_eq!(
        translated
            .chat
            .stream_options
            .as_ref()
            .map(|options| options.include_usage),
        Some(true)
    );
    Ok(())
}

#[test]
fn replays_plaintext_reasoning_and_forwards_the_cache_key_when_enabled() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "reasoning-test",
        "input": [
            {
                "type": "reasoning",
                "id": "rs_1",
                "summary": [],
                "content": [{"type": "reasoning_text", "text": "Inspect before answering."}],
                "encrypted_content": null
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "The answer."}]
            }
        ],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "prompt_cache_key": "thread-123",
        "stream": true
    }))?;
    let capabilities = BackendCapabilities {
        prompt_cache_key: PromptCacheKeyPolicy::Forward,
        reasoning_content: ReasoningContentPolicy::Plaintext,
        ..BackendCapabilities::default()
    };

    let translated = translate_request(request, capabilities, None)?;

    assert_eq!(
        translated.chat.messages,
        vec![ChatMessage::Assistant {
            content: Some("The answer.".to_string()),
            reasoning_content: Some("Inspect before answering.".to_string()),
            tool_calls: Vec::new(),
        }]
    );
    assert_eq!(
        translated.chat.prompt_cache_key,
        Some("thread-123".to_string())
    );
    Ok(())
}

#[test]
fn rejects_plaintext_reasoning_replay_without_backend_support() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "reasoning-test",
        "input": [{
            "type": "reasoning",
            "summary": [],
            "content": [{"type": "reasoning_text", "text": "private chain"}],
            "encrypted_content": null
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true
    }))?;

    let error = translate_request(request, BackendCapabilities::default(), None)
        .err()
        .map(|error| error.to_string());

    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("--supports-reasoning-content"))
    );
    Ok(())
}

#[test]
fn accepts_encrypted_reasoning_include_but_rejects_unknown_entries() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "test",
        "input": [],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true,
        "include": ["reasoning.encrypted_content"]
    }))?;

    translate_request(request, BackendCapabilities::default(), None)?;

    let request = parse_request(json!({
        "model": "test",
        "input": [],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true,
        "include": ["future.output"]
    }))?;
    let error = translate_request(request, BackendCapabilities::default(), None)
        .err()
        .map(|error| error.to_string());

    assert_eq!(
        error,
        Some(
            "unsupported Responses feature: Responses include entries [\"future.output\"]"
                .to_string()
        )
    );
    Ok(())
}

#[test]
fn translates_structured_output_and_declared_image_input() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "vision-test",
        "instructions": "",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                {"type": "input_text", "text": "Describe"},
                {"type": "input_image", "image_url": "data:image/png;base64,AA==", "detail": "high"}
            ]
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true,
        "text": {
            "format": {
                "type": "json_schema",
                "name": "answer",
                "strict": true,
                "schema": {"type": "object"}
            }
        }
    }))?;
    let capabilities = BackendCapabilities {
        image_input: true,
        structured_output: true,
        ..BackendCapabilities::default()
    };

    let translated = translate_request(request, capabilities, Some("upstream-model"))?;

    assert_eq!(translated.chat.model, "upstream-model");
    assert_eq!(
        translated.chat.messages,
        vec![ChatMessage::User {
            content: json!([
                {"type": "text", "text": "Describe"},
                {
                    "type": "image_url",
                    "image_url": {
                        "url": "data:image/png;base64,AA==",
                        "detail": "high"
                    }
                }
            ])
        }]
    );
    assert_eq!(
        translated.chat.response_format,
        Some(json!({
            "type": "json_schema",
            "json_schema": {
                "name": "answer",
                "strict": true,
                "schema": {"type": "object"}
            }
        }))
    );
    Ok(())
}

#[test]
fn rejects_image_input_without_declared_backend_support() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "vision-test",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_image", "image_url": "data:image/png;base64,AA=="}]
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true
    }))?;

    let error = translate_request(request, BackendCapabilities::default(), None)
        .err()
        .map(|error| error.to_string());

    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("--supports-image-input"))
    );
    Ok(())
}

#[test]
fn rejects_audio_content_for_every_message_role() -> anyhow::Result<()> {
    for (role, expected) in [
        ("user", "audio input"),
        ("assistant", "audio content in an assistant message"),
        ("developer", "audio content in a developer message"),
    ] {
        let request = parse_request(json!({
            "model": "audio-test",
            "input": [{
                "type": "message",
                "role": role,
                "content": [{"type": "input_audio", "audio_url": "data:audio/wav;base64,AA=="}]
            }],
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "stream": true
        }))?;

        let error = translate_request(request, BackendCapabilities::default(), None)
            .err()
            .map(|error| error.to_string());

        assert_eq!(
            error,
            Some(format!("unsupported Responses feature: {expected}"))
        );
    }
    Ok(())
}

#[test]
fn rejects_unknown_input_before_deserializing_to_response_item_other() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "test",
        "input": [{"type": "future_visible_item", "payload": "do not lose me"}],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true
    }))?;

    let error = translate_request(request, BackendCapabilities::default(), None)
        .err()
        .map(|error| error.to_string());

    assert_eq!(
        error.as_deref(),
        Some("unsupported Responses feature: input item type \"future_visible_item\"")
    );
    Ok(())
}

#[test]
fn rejects_parallel_calls_without_declared_backend_support() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "test",
        "input": [],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "stream": true
    }))?;

    let error = translate_request(request, BackendCapabilities::default(), None)
        .err()
        .map(|error| error.to_string());

    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("parallel_tool_calls=true"))
    );
    Ok(())
}

#[test]
fn rejects_orphaned_duplicate_and_unresolved_call_history() -> anyhow::Result<()> {
    let cases = [
        (
            vec![json!({
                "type": "function_call_output",
                "name": "send_message_to_thread",
                "output": "standalone output"
            })],
            "function call output history without call_id",
        ),
        (
            vec![json!({
                "type": "function_call_output",
                "call_id": "missing",
                "output": "no call"
            })],
            "unknown call ID \"missing\"",
        ),
        (
            vec![
                json!({
                    "type": "function_call",
                    "name": "shell",
                    "arguments": "{}",
                    "call_id": "duplicate"
                }),
                json!({
                    "type": "function_call",
                    "name": "shell",
                    "arguments": "{}",
                    "call_id": "duplicate"
                }),
            ],
            "duplicate tool call ID \"duplicate\"",
        ),
        (
            vec![json!({
                "type": "function_call",
                "name": "shell",
                "arguments": "{}",
                "call_id": "pending"
            })],
            "unresolved tool calls before the end of the request: pending",
        ),
    ];

    for (input, expected) in cases {
        let request = parse_request(json!({
            "model": "test",
            "input": input,
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "stream": true
        }))?;
        let error = translate_request(request, BackendCapabilities::default(), None)
            .err()
            .map(|error| error.to_string());
        assert!(
            error
                .as_deref()
                .is_some_and(|error| error.contains(expected)),
            "expected {expected:?}, received {error:?}"
        );
    }
    Ok(())
}

#[test]
fn disables_chat_stream_options_for_a_declared_non_streaming_backend() -> anyhow::Result<()> {
    let request = parse_request(json!({
        "model": "test",
        "input": [],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true
    }))?;
    let capabilities = BackendCapabilities {
        streaming: false,
        ..BackendCapabilities::default()
    };

    let translated = translate_request(request, capabilities, None)?;

    assert_eq!(translated.chat.stream, false);
    assert_eq!(translated.chat.stream_options, None);
    Ok(())
}
