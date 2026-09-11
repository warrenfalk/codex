use super::*;
use crate::app::test_support::make_test_app;
use crate::app_event::PromptRewriteCompletion;
use crate::bottom_pane::ComposerDraftSnapshot;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc::unbounded_channel;

fn request(parent_thread_id: ThreadId) -> PromptRewriteRequest {
    PromptRewriteRequest::new(
        parent_thread_id,
        ComposerDraftSnapshot {
            text: "wordy draft".to_string(),
            text_elements: Vec::new(),
            local_images: Vec::new(),
            remote_image_urls: Vec::new(),
            mention_bindings: Vec::new(),
            pending_pastes: Vec::new(),
            startup_local_history: Vec::new(),
            last_composer_activity_at: None,
            cursor: 0,
        },
        /*recognized_slash_command*/ false,
    )
    .expect("valid rewrite request")
}

#[tokio::test]
async fn fork_config_is_ephemeral_medium_and_appends_helper_policy() {
    let app = make_test_app().await;

    let config = app.prompt_rewrite_fork_config();

    assert!(config.ephemeral);
    assert_eq!(
        config.model_reasoning_effort,
        Some(ReasoningEffortConfig::Medium)
    );
    let instructions = config
        .developer_instructions
        .expect("rewrite developer instructions");
    assert!(instructions.contains("Your only task is to rewrite"));
    assert!(instructions.contains("Do not call tools"));
    let appended = App::prompt_rewrite_developer_instructions(Some("Existing developer policy."));
    assert!(appended.contains("Existing developer policy."));
    assert!(appended.contains("Your only task is to rewrite"));
}

#[tokio::test]
async fn completed_hidden_turn_applies_rewrite_without_registering_visible_routing() {
    let mut app = make_test_app().await;
    let parent_thread_id = ThreadId::new();
    let child_thread_id = ThreadId::new();
    app.active_thread_id = Some(parent_thread_id);
    app.chat_widget.insert_str("wordy draft");
    app.mark_prompt_rewrite_thread(child_thread_id);
    app.pending_prompt_rewrite = Some(PendingPromptRewrite { child_thread_id });
    let (sender, _receiver) = unbounded_channel();
    app.temporary_structured_requests
        .insert(child_thread_id, sender);

    app.finish_prompt_rewrite(PromptRewriteCompletion {
        child_thread_id,
        request: request(parent_thread_id),
        result: Ok(r#"{"rewritten_prompt":"Clear draft."}"#.to_string()),
    });

    assert_eq!(app.chat_widget.composer_text_with_pending(), "Clear draft.");
    assert!(app.pending_prompt_rewrite.is_none());
    assert!(
        !app.temporary_structured_requests
            .contains_key(&child_thread_id)
    );
    assert!(app.is_prompt_rewrite_thread(child_thread_id));
    assert!(!app.side_threads.contains_key(&child_thread_id));
    assert!(!app.thread_event_channels.contains_key(&child_thread_id));
}
