use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponsesRequest {
    pub model: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub input: Vec<Value>,
    pub tools: Option<Vec<Value>>,
    #[serde(default = "default_tool_choice")]
    pub tool_choice: Value,
    #[serde(default)]
    pub parallel_tool_calls: bool,
    pub reasoning: Option<ReasoningControls>,
    #[serde(default)]
    pub store: bool,
    pub stream: bool,
    pub stream_options: Option<Value>,
    #[serde(default)]
    pub include: Vec<String>,
    pub service_tier: Option<String>,
    pub prompt_cache_key: Option<String>,
    pub text: Option<TextControls>,
    pub client_metadata: Option<Value>,
}

fn default_tool_choice() -> Value {
    Value::String("auto".to_string())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReasoningControls {
    pub effort: Option<String>,
    pub summary: Option<Value>,
    pub context: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextControls {
    pub verbosity: Option<String>,
    pub format: Option<ResponsesTextFormat>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponsesTextFormat {
    #[serde(rename = "type")]
    pub kind: String,
    pub strict: bool,
    pub schema: Value,
    pub name: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    pub tool_choice: Value,
    pub parallel_tool_calls: bool,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<ChatStreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ChatStreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "role", rename_all = "lowercase")]
pub(crate) enum ChatMessage {
    System {
        content: String,
    },
    Developer {
        content: String,
    },
    User {
        content: Value,
    },
    Assistant {
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ChatToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct ChatToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: ChatFunctionCall,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct ChatFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChunk {
    pub id: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoice {
    pub index: usize,
    #[serde(default)]
    pub delta: ChatDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ChatDelta {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub refusal: Option<String>,
    pub tool_calls: Option<Vec<ChatToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub function: Option<ChatFunctionCallDelta>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatFunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) struct ChatUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub prompt_tokens_details: Option<ChatPromptTokenDetails>,
    pub completion_tokens_details: Option<ChatCompletionTokenDetails>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) struct ChatPromptTokenDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) struct ChatCompletionTokenDetails {
    #[serde(default)]
    pub reasoning_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BufferedChatCompletion {
    pub id: Option<String>,
    #[serde(default)]
    pub choices: Vec<BufferedChatChoice>,
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BufferedChatChoice {
    pub index: usize,
    pub message: BufferedChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BufferedChatMessage {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub refusal: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<BufferedChatToolCall>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BufferedChatToolCall {
    pub id: String,
    pub function: BufferedChatFunctionCall,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BufferedChatFunctionCall {
    pub name: String,
    pub arguments: String,
}
