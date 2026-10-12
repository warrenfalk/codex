use super::*;
use crate::legacy_core::config::ConfigBuilder;
use app_test_support::create_fake_rollout;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn side_fork_history_survives_server_restart_as_normal_fork() -> Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    let parent_id = ThreadId::from_string(
        &create_fake_rollout(
            home.path(),
            "2025-01-05T12-00-00",
            "2025-01-05T12:00:00Z",
            "Parent question",
            Some(config.model_provider_id.as_str()),
            /*git_info*/ None,
        )
        .expect("create parent rollout"),
    )?;
    let settings = LocalSettings::from(&config);
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let side = server
        .fork_side_thread(
            &settings,
            config.clone(),
            parent_id,
            /*selected_profile*/ None,
        )
        .await?;
    let side_id = side.session.thread_id;
    let fallback_name = format!("(Side) {side_id}");
    assert_eq!(side.session.thread_name, Some(fallback_name.clone()));
    let side_context = server
        .side_turn_context(side_id, "first-turn")
        .expect("side context");
    assert!(
        side_context["side_conversation"]
            .value
            .contains("For this turn only")
    );
    assert_ne!(
        server.side_turn_context(side_id, "second-turn"),
        Some(side_context)
    );
    server
        .thread_inject_items(
            side_id,
            vec![ResponseItem::Message {
                id: None,
                role: "assistant".into(),
                content: vec![ContentItem::OutputText {
                    text: "Side findings retained".into(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }],
        )
        .await?;
    let before = server.thread_read(side_id, /*include_turns*/ true).await?;
    assert!(!before.ephemeral);
    assert_eq!(before.name, Some(fallback_name.clone()));
    assert_eq!(before.forked_from_id, Some(parent_id.to_string()));
    server.shutdown().await?;
    let saved_history =
        std::fs::read_to_string(before.path.as_ref().expect("persisted side path"))?;
    assert!(saved_history.contains("Side findings retained"));

    let mut recovered = crate::start_embedded_app_server_for_picker(&config).await?;
    let after = recovered
        .thread_read(side_id, /*include_turns*/ true)
        .await?;
    assert_eq!(after.turns, before.turns);
    assert_eq!(after.forked_from_id, before.forked_from_id);
    assert_eq!(after.name, Some(fallback_name));
    let _ = recovered
        .resume_thread(
            &settings,
            config,
            side_id,
            ResumeModelSettings::RestoreFromThread,
        )
        .await?;
    assert_eq!(recovered.side_turn_context(side_id, "recovered-turn"), None);
    recovered.side_conversation_threads.insert(side_id);
    assert!(recovered.side_turn_context(side_id, "side-turn").is_some());
    recovered.side_conversation_threads.remove(&side_id);
    assert_eq!(recovered.side_turn_context(side_id, "kept-turn"), None);
    recovered.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn side_fork_copies_parent_name_once_and_allows_renaming_after_keep() -> Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    let parent_id = ThreadId::from_string(
        &create_fake_rollout(
            home.path(),
            "2025-01-05T12-00-00",
            "2025-01-05T12:00:00Z",
            "Parent question",
            Some(config.model_provider_id.as_str()),
            /*git_info*/ None,
        )
        .expect("create parent rollout"),
    )?;
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    server
        .thread_set_name(parent_id, "Original Name".into())
        .await?;
    let side = server
        .fork_side_thread(
            &LocalSettings::from(&config),
            config,
            parent_id,
            /*selected_profile*/ None,
        )
        .await?;
    let side_id = side.session.thread_id;
    assert_eq!(
        side.session.thread_name.as_deref(),
        Some("(Side) Original Name")
    );
    let saved = server.thread_read(side_id, /*include_turns*/ false).await?;
    insta::assert_snapshot!("side_fork_saved_name", saved.name.expect("saved side name"));
    server
        .thread_set_name(parent_id, "New parent name".into())
        .await?;
    let after_parent_rename = server.thread_read(side_id, /*include_turns*/ false).await?;
    assert_eq!(
        after_parent_rename.name,
        Some("(Side) Original Name".into())
    );
    server.side_conversation_threads.remove(&side_id);
    let kept = server.thread_read(side_id, /*include_turns*/ false).await?;
    assert_eq!(kept.name, after_parent_rename.name);
    server
        .thread_set_name(side_id, "Independent fork".into())
        .await?;
    let renamed = server.thread_read(side_id, /*include_turns*/ false).await?;
    assert_eq!(renamed.name, Some("Independent fork".into()));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn side_turn_guardrails_end_when_kept_or_recovered() -> Result<()> {
    use core_test_support::responses;
    let model_server = responses::start_mock_server().await;
    let home = tempfile::tempdir()?;
    let base_url = model_server.uri();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model = "gpt-5.2"
model_provider = "side-test"
[model_providers.side-test]
name = "OpenAI"
base_url = "{base_url}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )?;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    config.auto_thread_title = false;
    let settings = LocalSettings::from(&config);
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let started = server.start_thread(&config).await?;
    let thread_id = started.session.thread_id;
    server.side_conversation_threads.insert(thread_id);
    for stage in 0..3 {
        if stage == 1 {
            server.side_conversation_threads.remove(&thread_id);
        } else if stage == 2 {
            server.shutdown().await?;
            server = crate::start_embedded_app_server_for_picker(&config).await?;
            server
                .resume_thread(
                    &settings,
                    config.clone(),
                    thread_id,
                    ResumeModelSettings::RestoreFromThread,
                )
                .await?;
        }
        let mock = responses::mount_sse_once(
            &model_server,
            responses::sse(vec![
                responses::ev_response_created("response"),
                responses::ev_completed("response"),
            ]),
        )
        .await;
        server
            .turn_start(
                thread_id,
                format!("message-{stage}"),
                vec![UserInput::Text {
                    text: format!("Question {stage}"),
                    text_elements: Vec::new(),
                }],
                config.cwd.to_path_buf(),
                /*approval_policy*/ None,
                /*approvals_reviewer*/ None,
                TurnPermissionsOverride::Preserve,
                &config.workspace_roots,
                "gpt-5.2".into(),
                /*effort*/ None,
                /*summary*/ None,
                /*service_tier*/ None,
                /*collaboration_mode*/ None,
                /*output_schema*/ None,
                /*cyber_access_program*/ None,
            )
            .await?;
        tokio::time::timeout(Duration::from_secs(/*secs*/ 30), async {
            while let Some(event) = server.next_event().await {
                if let AppServerEvent::ServerNotification(notification) = event
                    && let codex_app_server_protocol::ServerNotification::TurnCompleted(completed) =
                        *notification
                {
                    assert_eq!(
                        completed.turn.status,
                        codex_app_server_protocol::TurnStatus::Completed,
                        "turn error: {:?}",
                        completed.turn.error,
                    );
                    return;
                }
            }
            panic!("disconnected before completing side test turn");
        })
        .await?;
        let request = mock.single_request();
        let texts = request.message_input_texts("developer");
        let side_contexts: Vec<_> = texts
            .iter()
            .filter(|text| text.contains("<side_conversation>"))
            .collect();
        // The original scoped instruction stays in history. Ordinary turns add none of their own.
        assert_eq!(side_contexts.len(), 1);
        assert!(side_contexts[0].contains("Side turn message-0."));
        assert!(
            side_contexts[0]
                .contains("These side-only restrictions expire when this turn finishes")
        );
    }
    server.shutdown().await?;
    Ok(())
}
