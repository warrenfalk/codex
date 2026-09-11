use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn translates_function_namespace_and_custom_tools() -> anyhow::Result<()> {
    let registry = ToolRegistry::from_responses_tools(Some(vec![
        json!({
            "type": "function",
            "name": "shell",
            "description": "Run a command",
            "strict": false,
            "parameters": {"type": "object"}
        }),
        json!({
            "type": "namespace",
            "name": "apps",
            "description": "Connected apps",
            "tools": [{
                "type": "function",
                "name": "search",
                "description": "Search mail",
                "strict": true,
                "parameters": {"type": "object", "properties": {}}
            }]
        }),
        json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply a patch",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: PATCH"
            }
        }),
    ]))?;

    let tools = registry
        .chat_tools()
        .ok_or_else(|| anyhow::anyhow!("expected translated tools"))?;
    assert_eq!(tools.len(), 3);
    assert_eq!(tools[0]["function"]["name"], json!("shell"));
    assert_eq!(
        tools[1]["function"]["description"],
        json!("Connected apps\n\nSearch mail")
    );
    assert!(
        tools[1]["function"]["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("codex_function_"))
    );
    assert!(
        tools[2]["function"]["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("codex_custom_"))
    );
    assert_eq!(
        tools[2]["function"]["parameters"],
        json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false
        })
    );
    Ok(())
}

#[test]
fn restores_original_custom_tool_call() -> anyhow::Result<()> {
    let registry = ToolRegistry::from_responses_tools(Some(vec![json!({
        "type": "custom",
        "name": "apply_patch",
        "description": "Apply",
        "format": {"type": "text", "syntax": "patch", "definition": ""}
    })]))?;
    let safe_name = registry.safe_name(&ToolIdentity::custom("apply_patch", None));

    let item = registry.response_item(
        "fc_1".to_string(),
        "call_1".to_string(),
        &safe_name,
        json!({"input": "*** Begin Patch"}).to_string(),
    )?;

    assert_eq!(
        item,
        json!({
            "id": "fc_1",
            "type": "custom_tool_call",
            "status": "completed",
            "call_id": "call_1",
            "name": "apply_patch",
            "namespace": null,
            "input": "*** Begin Patch"
        })
    );
    Ok(())
}

#[test]
fn rejects_tools_that_chat_cannot_execute() {
    let error = ToolRegistry::from_responses_tools(Some(vec![json!({
        "type": "web_search",
        "external_web_access": true
    })]))
    .err()
    .map(|error| error.to_string());

    assert_eq!(
        error.as_deref(),
        Some(
            "unsupported Responses feature: tool type \"web_search\" cannot be represented by Chat Completions"
        )
    );
}

#[test]
fn rejects_deferred_function_tools() {
    let error = ToolRegistry::from_responses_tools(Some(vec![json!({
        "type": "function",
        "name": "later",
        "description": "Deferred",
        "strict": false,
        "defer_loading": true,
        "parameters": {"type": "object"}
    })]))
    .err()
    .map(|error| error.to_string());

    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("deferred loading"))
    );
}
