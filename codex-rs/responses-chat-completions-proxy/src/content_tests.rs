use super::*;
use crate::config::ReasoningContentPolicy;
use crate::request::translate_request;
use pretty_assertions::assert_eq;

#[test]
fn image_tool_outputs_preserve_parallel_calls_reasoning_and_content_order() -> anyhow::Result<()> {
    // Both function and custom outputs share this path, including on history replay.
    for (call_type, output_type, arguments) in [
        (
            "function_call",
            "function_call_output",
            json!({"arguments": "{}"}),
        ),
        (
            "custom_tool_call",
            "custom_tool_call_output",
            json!({"input": "inspect"}),
        ),
    ] {
        let mut call = json!({"type": call_type, "name": "inspect", "call_id": "image"});
        call.as_object_mut()
            .unwrap()
            .extend(arguments.as_object().unwrap().clone());
        let request = serde_json::from_value(json!({
            "model": "vision", "stream": true,
            "input": [
                {"type": "reasoning", "summary": [], "content": [{"type": "reasoning_text", "text": "Inspect both."}]},
                call,
                {"type": "function_call", "name": "check", "call_id": "text", "arguments": "{}"},
                {"type": output_type, "call_id": "image", "output": [
                    {"type": "input_text", "text": "Before"},
                    {"type": "input_image", "image_url": "data:image/png;base64,first", "detail": "original"},
                    {"type": "input_text", "text": "Between"},
                    {"type": "input_image", "image_url": "data:image/png;base64,second"}
                ]},
                {"type": "function_call_output", "call_id": "text", "output": "OK"}
            ]
        }))?;
        let translated = translate_request(
            request,
            BackendCapabilities {
                image_input: true,
                image_tool_output: true,
                reasoning_content: ReasoningContentPolicy::Plaintext,
                ..Default::default()
            },
            /*upstream_model*/ None,
        )?;
        let messages = serde_json::to_value(&translated.chat.messages)?;
        let tool_name = messages[0]["tool_calls"][0]["function"]["name"].clone();
        assert_eq!(
            messages,
            json!([
                {"role": "assistant", "content": null, "reasoning_content": "Inspect both.", "tool_calls": [
                    {"id": "image", "type": "function", "function": {
                        "name": tool_name,
                        "arguments": if call_type == "function_call" { "{}" } else { "{\"input\":\"inspect\"}" }
                    }},
                    {"id": "text", "type": "function", "function": {"name": "check", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "image", "content": [
                    {"type": "text", "text": "Before"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,first", "detail": "original"}},
                    {"type": "text", "text": "Between"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,second"}}
                ]},
                {"role": "tool", "tool_call_id": "text", "content": "OK"}
            ])
        );
    }
    Ok(())
}

#[test]
fn structured_text_tool_outputs_do_not_require_image_support() -> anyhow::Result<()> {
    for (output, expected) in [
        (json!("unchanged"), "unchanged"),
        (json!([]), ""),
        (
            json!([{"type": "input_text", "text": "first"}, {"type": "input_text", "text": "second"}]),
            "first\nsecond",
        ),
    ] {
        assert_eq!(
            tool_content(
                serde_json::from_value(output)?,
                BackendCapabilities::default()
            )?,
            json!(expected)
        );
    }
    Ok(())
}

#[test]
fn user_image_support_does_not_imply_tool_image_support() -> anyhow::Result<()> {
    for capabilities in [
        BackendCapabilities {
            image_input: true,
            ..Default::default()
        },
        BackendCapabilities {
            image_tool_output: true,
            ..Default::default()
        },
    ] {
        let output = serde_json::from_value(
            json!([{ "type": "input_image", "image_url": "data:image/png;base64,image" }]),
        )?;
        assert!(tool_content(output, capabilities).is_err());
    }
    Ok(())
}

#[test]
fn unsupported_tool_parts_are_not_silently_dropped() -> anyhow::Result<()> {
    for part in [
        json!({"type": "input_audio", "audio_url": "data:audio/wav;base64,audio"}),
        json!({"type": "encrypted_content", "encrypted_content": "opaque"}),
    ] {
        let output =
            serde_json::from_value(json!([{ "type": "input_text", "text": "keep me" }, part]))?;
        assert!(
            tool_content(
                output,
                BackendCapabilities {
                    image_input: true,
                    image_tool_output: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    Ok(())
}
