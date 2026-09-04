use crate::error::ProxyError;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;

const MAX_CUSTOM_TOOL_DESCRIPTION_CHARS: usize = 32 * 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ToolIdentity {
    kind: ToolKind,
    namespace: Option<String>,
    name: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ToolKind {
    Function,
    Custom,
}

#[derive(Clone, Debug)]
struct OriginalTool {
    identity: ToolIdentity,
}

#[derive(Debug, Default)]
pub(crate) struct ToolRegistry {
    by_identity: HashMap<ToolIdentity, String>,
    by_safe_name: HashMap<String, OriginalTool>,
    chat_tools: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionTool {
    #[serde(rename = "type")]
    _kind: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    strict: bool,
    defer_loading: Option<bool>,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamespaceTool {
    #[serde(rename = "type")]
    _kind: String,
    name: String,
    #[serde(default)]
    description: String,
    tools: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomTool {
    #[serde(rename = "type")]
    _kind: String,
    name: String,
    #[serde(default)]
    description: String,
    format: CustomToolFormat,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomToolFormat {
    #[serde(rename = "type")]
    kind: String,
    syntax: String,
    definition: String,
}

impl ToolIdentity {
    pub(crate) fn function(name: &str, namespace: Option<&str>) -> Self {
        Self {
            kind: ToolKind::Function,
            namespace: namespace.map(str::to_string),
            name: name.to_string(),
        }
    }

    pub(crate) fn custom(name: &str, namespace: Option<&str>) -> Self {
        Self {
            kind: ToolKind::Custom,
            namespace: namespace.map(str::to_string),
            name: name.to_string(),
        }
    }
}

impl ToolRegistry {
    pub(crate) fn from_responses_tools(tools: Option<Vec<Value>>) -> Result<Self, ProxyError> {
        let mut registry = Self::default();
        for tool in tools.unwrap_or_default() {
            registry.add_tool(tool)?;
        }
        Ok(registry)
    }

    pub(crate) fn chat_tools(&self) -> Option<Vec<Value>> {
        (!self.chat_tools.is_empty()).then(|| self.chat_tools.clone())
    }

    pub(crate) fn safe_name(&self, identity: &ToolIdentity) -> String {
        self.by_identity
            .get(identity)
            .cloned()
            .unwrap_or_else(|| safe_name(identity))
    }

    pub(crate) fn response_item(
        &self,
        item_id: String,
        call_id: String,
        safe_name: &str,
        arguments: String,
    ) -> Result<Value, ProxyError> {
        let original = self.by_safe_name.get(safe_name).ok_or_else(|| {
            ProxyError::invalid_upstream(format!(
                "model called undeclared Chat function {safe_name:?}"
            ))
        })?;
        let ToolIdentity {
            kind,
            namespace,
            name,
        } = &original.identity;
        match kind {
            ToolKind::Function => Ok(json!({
                "id": item_id,
                "type": "function_call",
                "status": "completed",
                "call_id": call_id,
                "name": name,
                "namespace": namespace,
                "arguments": arguments,
            })),
            ToolKind::Custom => {
                let arguments: Value = serde_json::from_str(&arguments).map_err(|error| {
                    ProxyError::invalid_upstream(format!(
                        "custom tool {name:?} returned invalid JSON arguments: {error}"
                    ))
                })?;
                let input = arguments
                    .as_object()
                    .and_then(|arguments| arguments.get("input"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProxyError::invalid_upstream(format!(
                            "custom tool {name:?} arguments must contain a string field named input"
                        ))
                    })?;
                Ok(json!({
                    "id": item_id,
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": name,
                    "namespace": namespace,
                    "input": input,
                }))
            }
        }
    }

    fn add_tool(&mut self, value: Value) -> Result<(), ProxyError> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| ProxyError::invalid("tool is missing a string type discriminator"))?;
        match kind {
            "function" => {
                let tool: FunctionTool = serde_json::from_value(value).map_err(|error| {
                    ProxyError::invalid(format!("invalid function tool: {error}"))
                })?;
                self.add_function(tool, None, None)
            }
            "namespace" => {
                let namespace: NamespaceTool = serde_json::from_value(value).map_err(|error| {
                    ProxyError::invalid(format!("invalid namespace tool: {error}"))
                })?;
                for value in namespace.tools {
                    if value.get("type").and_then(Value::as_str) != Some("function") {
                        return Err(ProxyError::unsupported(format!(
                            "namespace {:?} contains a non-function tool",
                            namespace.name
                        )));
                    }
                    let tool: FunctionTool = serde_json::from_value(value).map_err(|error| {
                        ProxyError::invalid(format!(
                            "invalid function in namespace {:?}: {error}",
                            namespace.name
                        ))
                    })?;
                    self.add_function(tool, Some(&namespace.name), Some(&namespace.description))?;
                }
                Ok(())
            }
            "custom" => {
                let tool: CustomTool = serde_json::from_value(value).map_err(|error| {
                    ProxyError::invalid(format!("invalid custom tool: {error}"))
                })?;
                self.add_custom(tool)
            }
            unsupported => Err(ProxyError::unsupported(format!(
                "tool type {unsupported:?} cannot be represented by Chat Completions"
            ))),
        }
    }

    fn add_function(
        &mut self,
        tool: FunctionTool,
        namespace: Option<&str>,
        namespace_description: Option<&str>,
    ) -> Result<(), ProxyError> {
        if tool.defer_loading.unwrap_or(false) {
            return Err(ProxyError::unsupported(format!(
                "deferred loading for function tool {:?}; disable tool_search/deferred tools in the Codex model profile",
                tool.name
            )));
        }
        let identity = ToolIdentity::function(&tool.name, namespace);
        let safe_name = self.register(identity)?;
        let description = match namespace_description {
            Some(namespace_description) if !namespace_description.is_empty() => {
                format!("{namespace_description}\n\n{}", tool.description)
            }
            _ => tool.description,
        };
        self.chat_tools.push(json!({
            "type": "function",
            "function": {
                "name": safe_name,
                "description": description,
                "parameters": tool.parameters,
                "strict": tool.strict,
            }
        }));
        Ok(())
    }

    fn add_custom(&mut self, tool: CustomTool) -> Result<(), ProxyError> {
        let identity = ToolIdentity::custom(&tool.name, None);
        let safe_name = self.register(identity)?;
        let format_description = format!(
            "Freeform input format: type={}, syntax={}.\n{}",
            tool.format.kind, tool.format.syntax, tool.format.definition
        );
        let description = if tool.description.is_empty() {
            format_description
        } else {
            format!("{}\n\n{format_description}", tool.description)
        };
        if description.chars().count() > MAX_CUSTOM_TOOL_DESCRIPTION_CHARS {
            return Err(ProxyError::unsupported(format!(
                "custom tool {:?} requires a generated description longer than the {MAX_CUSTOM_TOOL_DESCRIPTION_CHARS}-character V1 limit",
                tool.name
            )));
        }
        self.chat_tools.push(json!({
            "type": "function",
            "function": {
                "name": safe_name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "input": {"type": "string"}
                    },
                    "required": ["input"],
                    "additionalProperties": false
                },
                "strict": true
            }
        }));
        Ok(())
    }

    fn register(&mut self, identity: ToolIdentity) -> Result<String, ProxyError> {
        if self.by_identity.contains_key(&identity) {
            return Err(ProxyError::invalid(format!(
                "tool is declared more than once: {identity:?}"
            )));
        }
        let safe_name = safe_name(&identity);
        if let Some(existing) = self.by_safe_name.get(&safe_name) {
            return Err(ProxyError::invalid(format!(
                "tool name collision between {:?} and {identity:?}",
                existing.identity
            )));
        }
        self.by_identity.insert(identity.clone(), safe_name.clone());
        self.by_safe_name
            .insert(safe_name.clone(), OriginalTool { identity });
        Ok(safe_name)
    }
}

fn safe_name(identity: &ToolIdentity) -> String {
    if identity.kind == ToolKind::Function
        && identity.namespace.is_none()
        && is_chat_safe_name(&identity.name)
    {
        return identity.name.clone();
    }
    let kind = match identity.kind {
        ToolKind::Function => "function",
        ToolKind::Custom => "custom",
    };
    let source = format!(
        "{kind}\0{}\0{}",
        identity.namespace.as_deref().unwrap_or_default(),
        identity.name
    );
    let digest = Sha256::digest(source.as_bytes());
    let suffix = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("codex_{kind}_{suffix}")
}

fn is_chat_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
#[path = "tool_registry_tests.rs"]
mod tests;
