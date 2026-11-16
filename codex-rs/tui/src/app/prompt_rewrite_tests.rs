use super::*;
use crate::app::test_support::make_test_app;
use crate::app_event::PromptRewriteCompletion;
use crate::bottom_pane::ComposerDraftSnapshot;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn helper_creation_is_ephemeral_before_history_and_inherits_saved_context() -> Result<()> {
    let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
    let model = core_test_support::responses::start_mock_server().await;
    app_test_support::MockResponsesConfig::new(&model.uri()).write(&app.config.codex_home)?;
    app.config = app
        .rebuild_config_for_cwd(app.config.cwd.to_path_buf())
        .await?;
    app.config.auto_thread_title = false;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let parent = server.start_thread(&app.config).await?.session.thread_id;
    // Creation must enforce helper isolation even from ordinary launch defaults.
    let config = app.config.clone();
    let fresh = server
        .create_prompt_rewrite_thread(&config, parent, Some(parent))
        .await?;
    assert!(
        server
            .thread_read(fresh, /*include_turns*/ false)
            .await?
            .ephemeral
    );
    server
        .thread_inject_items(
            parent,
            vec![codex_protocol::models::ResponseItem::Message {
                status: None,
                encrypted_content: None,
                id: None,
                role: "assistant".to_string(),
                content: vec![codex_protocol::models::ContentItem::OutputText {
                    annotations: None,
                    logprobs: None,
                    text: "Earlier saved context.".to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }],
        )
        .await?;
    let child = server
        .create_prompt_rewrite_thread(&config, parent, Some(parent))
        .await?;
    assert_ne!(child, fresh);
    assert!(
        server
            .thread_read(child, /*include_turns*/ false)
            .await?
            .ephemeral
    );
    let response = core_test_support::responses::mount_sse_once(
        &model,
        core_test_support::responses::sse(vec![
            core_test_support::responses::ev_response_created("rewrite"),
            core_test_support::responses::ev_assistant_message(
                "answer",
                r#"{"rewritten_prompt":"Clear."}"#,
            ),
            core_test_support::responses::ev_completed("rewrite"),
        ]),
    )
    .await;
    server
        .turn_start_model_only(
            child,
            vec![codex_app_server_protocol::UserInput::Text {
                text: "Rewrite this.".to_string(),
                text_elements: Vec::new(),
            }],
            "mock-model".to_string(),
            Some(ReasoningEffortConfig::Medium),
            /*output_schema*/ None,
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
        loop {
            let event = server.next_event().await.expect("server event");
            if let codex_app_server_client::AppServerEvent::ServerNotification(notification) =
                event.event
                && let ServerNotification::TurnCompleted(done) = notification.as_ref()
                && done.thread_id == child.to_string()
            {
                break;
            }
        }
    })
    .await?;
    let request = response.single_request().body_json();
    assert_eq!(request["tools"], serde_json::json!([]));
    assert!(
        request["input"]
            .to_string()
            .contains("Earlier saved context.")
    );
    server.shutdown().await?;
    Ok(())
}

fn request(parent_thread_id: ThreadId) -> PromptRewriteRequest {
    PromptRewriteRequest::new(
        parent_thread_id,
        ComposerDraftSnapshot {
            vim_normal_mode: false,
            shell_mode: None,
            text: "wordy draft".to_string(),
            text_elements: Vec::new(),
            local_images: Vec::new(),
            remote_image_urls: Vec::new(),
            mention_bindings: Vec::new(),
            pending_pastes: Vec::new(),
            startup_local_history: Vec::new(),
            last_composer_activity_at: None,
            sparkle_draft: Default::default(),
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
