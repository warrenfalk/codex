use std::sync::Arc;

use codex_api::ResponseEvent;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::items::TurnItem;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::protocol::USER_MESSAGE_BEGIN;
use codex_rollout_trace::InferenceTraceContext;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tracing::warn;

use crate::Prompt;
use crate::parse_turn_item;
use crate::responses_metadata::CodexResponsesRequestKind;
use crate::session::session::Session;
use crate::session::thread_title_from_thread_store;
use crate::session::turn_context::TurnContext;

const AUTO_THREAD_TITLE_PROMPT: &str = "\
Write a concise title for this Codex thread.

Requirements:
- Return JSON that matches the provided schema.
- Use plain text only.
- Keep it under 8 words.
- Capture the user's main task, not the assistant's behavior.
- Do not include quotes, prefixes, or trailing punctuation.
- Do not mention Codex, the assistant, or that this is a thread title.";

const MAX_PROMPT_TEXT_CHARS: usize = 1_200;
const DISABLE_AUTO_THREAD_TITLE_FOR_TESTS_ENV_VAR: &str =
    "CODEX_DISABLE_AUTO_THREAD_TITLE_FOR_TESTS";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThreadTitleOutput {
    title: String,
}

pub(crate) async fn maybe_generate_and_set_thread_title(
    session: Arc<Session>,
    turn_context: Arc<TurnContext>,
    last_agent_message: Option<String>,
) {
    if has_distinct_thread_title(session.as_ref()).await {
        return;
    }

    let Some(first_user_message) = first_user_message(session.as_ref()).await else {
        return;
    };
    let Some(normalized_first_user_message) =
        crate::util::normalize_thread_name(&first_user_message)
    else {
        return;
    };

    let generated_title = match generate_thread_title(
        session.as_ref(),
        turn_context.as_ref(),
        &first_user_message,
        last_agent_message.as_deref(),
    )
    .await
    {
        Ok(Some(title)) => title,
        Ok(None) => return,
        Err(err) => {
            warn!("failed to auto-generate thread title: {err:#}");
            return;
        }
    };

    let Some(generated_title) = crate::util::normalize_thread_name(&generated_title) else {
        return;
    };
    if generated_title == normalized_first_user_message {
        return;
    }

    match Session::apply_thread_name_update_if_unnamed(&session, generated_title).await {
        Ok(true) | Ok(false) => {}
        Err(err) => warn!("failed to persist auto-generated thread title: {err:#}"),
    }
}

pub(crate) fn auto_thread_title_disabled_for_tests() -> bool {
    cfg!(debug_assertions)
        && std::env::var_os(DISABLE_AUTO_THREAD_TITLE_FOR_TESTS_ENV_VAR).is_some()
}

async fn generate_thread_title(
    session: &Session,
    turn_context: &TurnContext,
    first_user_message: &str,
    last_agent_message: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let assistant_section = last_agent_message
        .map(strip_user_message_prefix)
        .filter(|text| !text.is_empty())
        .map(|text| {
            format!(
                "\nLatest assistant reply:\n{}",
                truncate_for_prompt(text, MAX_PROMPT_TEXT_CHARS)
            )
        })
        .unwrap_or_default();

    let prompt = Prompt {
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: format!(
                    "First user message:\n{}{}",
                    truncate_for_prompt(first_user_message, MAX_PROMPT_TEXT_CHARS),
                    assistant_section,
                ),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }],
        tools: Vec::new(),
        parallel_tool_calls: false,
        base_instructions: BaseInstructions {
            text: AUTO_THREAD_TITLE_PROMPT.to_string(),
        },
        output_schema: Some(output_schema()),
        output_schema_strict: true,
    };

    let inference_trace = InferenceTraceContext::disabled();
    let mut client_session = session.services.model_client.new_session();
    let window_id = session.current_window_id().await;
    let responses_metadata = turn_context.turn_metadata_state.to_responses_metadata(
        session.installation_id.clone(),
        window_id,
        CodexResponsesRequestKind::Turn,
    );
    let mut stream = client_session
        .stream(
            &prompt,
            &turn_context.model_info,
            &turn_context.session_telemetry,
            Some(ReasoningEffortConfig::Low),
            ReasoningSummaryConfig::None,
            turn_context.config.service_tier.clone(),
            &responses_metadata,
            &inference_trace,
        )
        .await?;

    let mut result = String::new();
    while let Some(message) = stream.next().await.transpose()? {
        match message {
            ResponseEvent::OutputTextDelta(delta) => result.push_str(&delta),
            ResponseEvent::OutputItemDone(item) => {
                if result.is_empty()
                    && let ResponseItem::Message { content, .. } = item
                    && let Some(text) = crate::compact::content_items_to_text(&content)
                {
                    result.push_str(&text);
                }
            }
            ResponseEvent::Completed { .. } => break,
            _ => {}
        }
    }

    if result.trim().is_empty() {
        return Ok(None);
    }

    let output: ThreadTitleOutput = serde_json::from_str(&result)?;
    Ok(Some(output.title))
}

async fn first_user_message(session: &Session) -> Option<String> {
    let history = session.clone_history().await;
    if let Some(first_user_message) =
        history
            .raw_items()
            .iter()
            .find_map(|item| match parse_turn_item(item) {
                Some(TurnItem::UserMessage(user_message)) => Some(user_message.message()),
                _ => None,
            })
    {
        let first_user_message = strip_user_message_prefix(&first_user_message).to_string();
        if !first_user_message.is_empty() {
            return Some(first_user_message);
        }
    }

    if let Ok(thread) = session
        .services
        .thread_store
        .read_thread(codex_thread_store::ReadThreadParams {
            thread_id: session.thread_id(),
            include_archived: true,
            include_history: false,
        })
        .await
        && let Some(first_user_message) = thread.first_user_message
    {
        let first_user_message = strip_user_message_prefix(&first_user_message).to_string();
        if !first_user_message.is_empty() {
            return Some(first_user_message);
        }
    }

    None
}

async fn has_distinct_thread_title(session: &Session) -> bool {
    thread_title_from_thread_store(
        session.live_thread(),
        &session.services.thread_store,
        session.thread_id(),
    )
    .await
    .is_some()
}

fn output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "description": "A concise thread title."
            }
        },
        "required": ["title"],
        "additionalProperties": false
    })
}

fn strip_user_message_prefix(text: &str) -> &str {
    match text.find(USER_MESSAGE_BEGIN) {
        Some(idx) => text[idx + USER_MESSAGE_BEGIN.len()..].trim(),
        None => text.trim(),
    }
}

fn truncate_for_prompt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let truncated = trimmed.chars().take(max_chars).collect::<String>();
    if trimmed.chars().count() > max_chars {
        format!("{truncated}...")
    } else {
        truncated
    }
}
