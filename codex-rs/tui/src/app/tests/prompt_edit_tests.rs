//! Prompt-edit choices and persistence through the public app-server API.

use super::*;
use crate::app_server_session::ResumeModelSettings;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::user_input::UserInput as CoreUserInput;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn prompt_edit_menu_defaults_to_replacement_and_can_cancel() {
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let thread_id = ThreadId::new();
    app.chat_widget
        .handle_thread_session(test_thread_session(thread_id, app.config.cwd.to_path_buf()));
    app.chat_widget
        .apply_external_edit("keep this draft".to_string());
    let selection = BacktrackSelection {
        thread_id,
        nth_user_message: 1,
        prompt: crate::chatwidget::UserMessage::from("selected prompt"),
    };
    while events.try_recv().is_ok() {}
    app.apply_backtrack_selection(selection.clone());
    assert!(events.try_recv().is_err());
    insta::assert_snapshot!(
        "prompt_edit_menu",
        render_bottom_popup(&app.chat_widget, /*width*/ 100)
    );
    insta::assert_snapshot!(
        "prompt_edit_menu_narrow",
        render_bottom_popup(&app.chat_widget, /*width*/ 45)
    );
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(events.try_recv().is_err());
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "keep this draft"
    );

    app.apply_backtrack_selection(selection.clone());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let event = std::iter::from_fn(|| events.try_recv().ok())
        .find(|event| matches!(event, AppEvent::RevertSessionForPromptEdit { .. }))
        .expect("the default choice should replace this conversation");
    let AppEvent::RevertSessionForPromptEdit {
        thread_id,
        nth_user_message,
        prompt,
    } = event
    else {
        unreachable!();
    };
    assert_eq!(
        BacktrackSelection {
            thread_id,
            nth_user_message,
            prompt
        },
        selection
    );
}

async fn create_prompt_edit_rollout(
    config: &Config,
    history_mode: ThreadHistoryMode,
) -> Result<(ThreadId, PathBuf)> {
    let create = match history_mode {
        ThreadHistoryMode::Legacy => app_test_support::create_fake_rollout,
        ThreadHistoryMode::Paginated => app_test_support::create_fake_paginated_rollout,
    };
    let id = create(
        config.codex_home.as_path(),
        "2025-01-05T12-00-00",
        "2025-01-05T12:00:00Z",
        "unused",
        Some(config.model_provider_id.as_str()),
        /*git_info*/ None,
    )
    .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
    let path =
        app_test_support::rollout_path(config.codex_home.as_path(), "2025-01-05T12-00-00", &id);
    let contents = std::fs::read_to_string(&path)?;
    let meta = contents.lines().next().expect("session metadata");
    std::fs::write(&path, format!("{meta}\n"))?;
    for (turn_id, messages) in [
        ("turn-1", vec!["retained prompt"]),
        ("turn-2", vec!["selected prompt [Image #1]"]),
        ("turn-3", vec!["discarded prompt", "discarded steer"]),
    ] {
        codex_rollout::append_rollout_item_to_path(
            &path,
            &RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: turn_id.to_string(),
                trace_id: None,
                started_at: None,
                model_context_window: None,
                collaboration_mode_kind: ModeKind::default(),
            })),
        )
        .await?;
        for message in messages {
            let input = ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: message.to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            };
            codex_rollout::append_rollout_item_to_path(
                &path,
                &RolloutItem::ResponseItem(input.into()),
            )
            .await?;
            let user_event = if history_mode == ThreadHistoryMode::Paginated {
                let mut content = vec![CoreUserInput::Text {
                    text: message.to_string(),
                    text_elements: Vec::new(),
                }];
                if turn_id == "turn-2" {
                    content.push(CoreUserInput::Image {
                        image_url: "https://example.com/image.png".to_string(),
                        detail: None,
                    });
                    content.push(CoreUserInput::LocalImage {
                        path: test_path_buf("/tmp/prompt-image.png"),
                        detail: None,
                    });
                }
                EventMsg::ItemCompleted(ItemCompletedEvent {
                    thread_id: ThreadId::from_string(&id)?,
                    turn_id: turn_id.to_string(),
                    item: TurnItem::UserMessage(UserMessageItem {
                        id: message.to_string(),
                        client_id: None,
                        content,
                    }),
                    started_at_ms: None,
                    completed_at_ms: 0,
                })
            } else {
                EventMsg::UserMessage(UserMessageEvent {
                    message: message.to_string(),
                    images: (turn_id == "turn-2")
                        .then(|| vec!["https://example.com/image.png".to_string()]),
                    local_images: if turn_id == "turn-2" {
                        vec![test_path_buf("/tmp/prompt-image.png")]
                    } else {
                        Vec::new()
                    },
                    ..Default::default()
                })
            };
            codex_rollout::append_rollout_item_to_path(&path, &RolloutItem::EventMsg(user_event))
                .await?;
        }
        codex_rollout::append_rollout_item_to_path(
            &path,
            &RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: turn_id.to_string(),
                last_agent_message: None,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                time_to_first_token_ms: None,
            })),
        )
        .await?;
    }
    Ok((ThreadId::from_string(&id)?, path))
}

#[tokio::test]
async fn prompt_edit_replacement_keeps_identity_and_persists_in_both_history_formats() -> Result<()>
{
    for history_mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        for selected in [0, 1] {
            let (mut app, mut events, _ops) = make_test_app_with_channels().await;
            let mut config = app.config.clone();
            config
                .features
                .disable(Feature::BackgroundPaginatedRolloutMigration)?;
            let (thread_id, original_path) =
                create_prompt_edit_rollout(&config, history_mode).await?;
            let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&config)).await?;
            server.remember_thread_history_mode(thread_id, history_mode);
            let started = server
                .resume_thread(
                    &app.local_settings,
                    config.clone(),
                    thread_id,
                    ResumeModelSettings::RestoreFromThread,
                )
                .await?;
            assert_eq!(
                server
                    .thread_read(thread_id, /*include_turns*/ false)
                    .await?
                    .history_mode,
                history_mode
            );
            let expected_turns = started.turns[..selected].to_vec();
            let expected_read_turns = server
                .thread_read(thread_id, /*include_turns*/ true)
                .await?
                .turns[..selected]
                .to_vec();
            app.enqueue_primary_thread_session(started.session, started.turns)
                .await?;
            while events.try_recv().is_ok() {}
            let original_contents = std::fs::read_to_string(&original_path)?;
            let prompt = if selected == 0 {
                crate::chatwidget::UserMessage::from("retained prompt")
            } else {
                crate::chatwidget::UserMessage {
                    text: "selected prompt [Image #1]".to_string(),
                    local_images: vec![crate::bottom_pane::LocalImageAttachment {
                        placeholder: "[Image #1]".to_string(),
                        path: test_path_buf("/tmp/prompt-image.png"),
                    }],
                    remote_image_urls: vec!["https://example.com/image.png".to_string()],
                    text_elements: Vec::new(),
                    mention_bindings: Vec::new(),
                }
            };
            let mut tui = crate::tui::test_support::make_test_tui()?;
            // A display insert queued before completion must not survive the transcript rebuild.
            app.app_event_tx
                .send(AppEvent::InsertHistoryCell(Box::new(UserHistoryCell {
                    message: "discarded buffered history".into(),
                    text_elements: Vec::new(),
                    local_image_paths: Vec::new(),
                    remote_image_urls: Vec::new(),
                    spoken: false,
                })));
            Box::pin(app.handle_event(
                &mut tui,
                &mut server,
                AppEvent::RevertSessionForPromptEdit {
                    thread_id,
                    nth_user_message: selected,
                    prompt: prompt.clone(),
                },
            ))
            .await?;
            let mut completed = false;
            while let Ok(event) = events.try_recv() {
                completed |= matches!(event, AppEvent::PromptEditReverted { .. });
                Box::pin(app.handle_event(&mut tui, &mut server, event)).await?;
            }
            assert!(
                completed,
                "replacement should succeed: {:?}",
                app.transcript_cells
                    .iter()
                    .map(|cell| lines_to_single_string(&cell.display_lines(/*width*/ 100)))
                    .collect::<Vec<_>>()
            );
            assert_eq!(app.chat_widget.thread_id(), Some(thread_id));
            assert_eq!(app.chat_widget.composer_text_with_pending(), prompt.text);
            assert_eq!(
                app.chat_widget.remote_image_urls(),
                prompt.remote_image_urls
            );
            assert_eq!(
                server
                    .thread_read(thread_id, /*include_turns*/ true)
                    .await?
                    .turns,
                expected_read_turns
            );
            {
                let store = app.thread_event_channels[&thread_id].store.lock().await;
                assert_eq!(store.turns, expected_turns);
                assert!(store.buffer.is_empty());
            }
            assert!(!server.has_older_history(thread_id));
            let history = app
                .transcript_cells
                .iter()
                .map(|cell| lines_to_single_string(&cell.display_lines(/*width*/ 100)))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!history.contains("discarded"), "{history}");
            assert!(history.contains("You’re editing this conversation from this point"));
            if history_mode == ThreadHistoryMode::Paginated {
                if selected == 1 {
                    insta::assert_snapshot!(
                        "prompt_edit_replacement_notice",
                        lines_to_single_string(
                            &app.transcript_cells
                                .last()
                                .expect("replacement notice")
                                .display_lines(/*width*/ 100),
                        )
                    );
                }
                assert_eq!(std::fs::read_to_string(&original_path)?, original_contents);
            } else {
                let saved = std::fs::read_to_string(&original_path)?;
                assert!(saved.starts_with(&original_contents));
                assert!(saved.contains("thread_rolled_back"));
            }
            server.shutdown().await?;
            let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&config)).await?;
            server.remember_thread_history_mode(thread_id, history_mode);
            let resumed = server
                .resume_thread(
                    &app.local_settings,
                    config.clone(),
                    thread_id,
                    ResumeModelSettings::RestoreFromThread,
                )
                .await?;
            assert_eq!(resumed.session.thread_id, thread_id);
            assert_eq!(resumed.turns, expected_turns);
            server.shutdown().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn prompt_edit_replacement_failure_preserves_history_and_prompt() -> Result<()> {
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let config = app.config.clone();
    let (thread_id, _) = create_prompt_edit_rollout(&config, ThreadHistoryMode::Paginated).await?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&config)).await?;
    let started = server
        .resume_thread(
            &app.local_settings,
            config.clone(),
            thread_id,
            ResumeModelSettings::RestoreFromThread,
        )
        .await?;
    let expected_turns = server
        .thread_read(thread_id, /*include_turns*/ true)
        .await?
        .turns;
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    while events.try_recv().is_ok() {}
    // A stale selection must never truncate a different prompt.
    let prompt = crate::chatwidget::UserMessage::from("outdated selected prompt");
    let mut tui = crate::tui::test_support::make_test_tui()?;
    Box::pin(app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::RevertSessionForPromptEdit {
            thread_id,
            nth_user_message: 1,
            prompt: prompt.clone(),
        },
    ))
    .await?;
    assert_eq!(app.chat_widget.composer_text_with_pending(), prompt.text);
    assert_eq!(
        server
            .thread_read(thread_id, /*include_turns*/ true)
            .await?
            .turns,
        expected_turns
    );
    insta::assert_snapshot!(
        "prompt_edit_replacement_failure",
        next_history_message(&mut events)
    );
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, AppEvent::PromptEditReverted { .. }))
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn prompt_edit_replacement_server_failure_preserves_selected_prompt() -> Result<()> {
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let config = app.config.clone();
    let (thread_id, _) = create_prompt_edit_rollout(&config, ThreadHistoryMode::Paginated).await?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&config)).await?;
    let started = server
        .resume_thread(
            &app.local_settings,
            config.clone(),
            thread_id,
            ResumeModelSettings::RestoreFromThread,
        )
        .await?;
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    while events.try_recv().is_ok() {}
    // Another client has already replaced this history, so our cached turn ID is stale.
    let externally_reverted = server
        .revert_thread_before(thread_id, "turn-1".to_string())
        .await?;
    assert!(externally_reverted.turns.is_empty());
    let prompt = crate::chatwidget::UserMessage::from("retained prompt");
    let mut tui = crate::tui::test_support::make_test_tui()?;
    Box::pin(app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::RevertSessionForPromptEdit {
            thread_id,
            nth_user_message: 0,
            prompt: prompt.clone(),
        },
    ))
    .await?;
    assert_eq!(app.chat_widget.thread_id(), Some(thread_id));
    assert_eq!(app.chat_widget.composer_text_with_pending(), prompt.text);
    assert!(
        server
            .thread_read(thread_id, /*include_turns*/ true)
            .await?
            .turns
            .is_empty()
    );
    let error = next_history_message(&mut events);
    assert!(
        error.contains("Failed to replace the conversation"),
        "{error}"
    );
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, AppEvent::PromptEditReverted { .. }))
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn prompt_edit_forks_before_selected_prompt_and_preserves_source() -> Result<()> {
    let (mut app, mut app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let config = app.chat_widget.config_ref().clone();
    let filename_ts = "2025-01-05T12-00-00";
    let source_thread_id = app_test_support::create_fake_rollout(
        config.codex_home.as_path(),
        filename_ts,
        "2025-01-05T12:00:00Z",
        "unused preview",
        Some("test-provider"),
        /*git_info*/ None,
    )
    .expect("materialized rollout should be created");
    let source_path =
        app_test_support::rollout_path(config.codex_home.as_path(), filename_ts, &source_thread_id);
    let session_meta = std::fs::read_to_string(&source_path)?
        .lines()
        .next()
        .expect("fake rollout should have session metadata")
        .to_string();
    std::fs::write(&source_path, format!("{session_meta}\n"))?;
    for (turn_id, message, images, local_images) in [
        ("turn-1", "retained prompt", None, Vec::new()),
        (
            "turn-2",
            "selected prompt [Image #1]",
            Some(vec!["https://example.com/backtrack.png".to_string()]),
            vec![PathBuf::from("/tmp/fake-image.png")],
        ),
    ] {
        for item in [
            RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: turn_id.to_string(),
                trace_id: None,
                started_at: None,
                model_context_window: None,
                collaboration_mode_kind: ModeKind::default(),
            })),
            RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
                message: message.to_string(),
                images,
                local_images,
                ..Default::default()
            })),
            RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: turn_id.to_string(),
                last_agent_message: None,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                time_to_first_token_ms: None,
            })),
        ] {
            codex_rollout::append_rollout_item_to_path(&source_path, &item).await?;
        }
    }

    let source_thread_id = ThreadId::from_string(&source_thread_id)?;
    let mut app_server = Box::pin(crate::start_embedded_app_server_for_picker(&config)).await?;
    let started = app_server
        .resume_thread(
            &crate::local_settings::LocalSettings::from(&config),
            config.clone(),
            source_thread_id,
            crate::app_server_session::ResumeModelSettings::OverrideFromCurrentConfig,
        )
        .await?;
    let selected_turn = started.turns[1].clone();
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    {
        let mut store = app
            .thread_event_channels
            .get(&source_thread_id)
            .expect("source thread event channel")
            .store
            .lock()
            .await;
        store.turns.pop();
        store.push_notification(turn_started_notification(
            source_thread_id,
            &selected_turn.id,
        ));
        for item in selected_turn.items {
            store.push_notification(ServerNotification::ItemCompleted(
                codex_app_server_protocol::ItemCompletedNotification {
                    thread_id: source_thread_id.to_string(),
                    turn_id: selected_turn.id.clone(),
                    completed_at_ms: 0,
                    item,
                },
            ));
        }
        store.push_notification(turn_completed_notification(
            source_thread_id,
            &selected_turn.id,
            TurnStatus::Interrupted,
        ));
    }
    while app_event_rx.try_recv().is_ok() {}
    let source_before = std::fs::read_to_string(&source_path)?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let prompt = crate::chatwidget::UserMessage {
        text: "selected prompt [Image #1]".to_string(),
        local_images: vec![crate::bottom_pane::LocalImageAttachment {
            placeholder: "[Image #1]".to_string(),
            path: PathBuf::from("/tmp/fake-image.png"),
        }],
        remote_image_urls: vec!["https://example.com/backtrack.png".to_string()],
        text_elements: Vec::new(),
        mention_bindings: Vec::new(),
    };

    let control = Box::pin(app.handle_event(
        &mut tui,
        &mut app_server,
        AppEvent::ForkSessionForPromptEdit {
            thread_id: source_thread_id,
            nth_user_message: 1,
            prompt: prompt.clone(),
        },
    ))
    .await?;

    assert!(matches!(control, AppRunControl::Continue));
    let forked_thread_id = app
        .chat_widget
        .thread_id()
        .expect("prompt edit should switch to a forked thread");
    assert_ne!(forked_thread_id, source_thread_id);
    assert_eq!(app.chat_widget.composer_text_with_pending(), prompt.text);
    assert_eq!(
        app.chat_widget.remote_image_urls(),
        prompt.remote_image_urls
    );
    assert_eq!(std::fs::read_to_string(&source_path)?, source_before);
    assert_eq!(
        app_server
            .thread_read(source_thread_id, /*include_turns*/ true)
            .await?
            .turns
            .iter()
            .map(|turn| turn.id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-1", "turn-2"]
    );
    assert_eq!(
        app_server
            .thread_read(forked_thread_id, /*include_turns*/ true)
            .await?
            .turns
            .iter()
            .map(|turn| turn.id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-1"]
    );

    let history = std::iter::from_fn(|| app_event_rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::InsertHistoryCell(cell) => {
                Some(lines_to_single_string(&cell.display_lines(/*width*/ 120)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let retained_index = history
        .iter()
        .position(|line| line.contains("retained prompt"))
        .expect("forked history should replay the retained prompt");
    let notice_index = history
        .iter()
        .position(|line| line == "• You’re continuing from this point in a new conversation")
        .expect("prompt edit should emit the branch notice");
    assert!(retained_index < notice_index);
    assert!(
        !history
            .iter()
            .any(|line| line.contains("Thread forked from"))
    );
    app_server.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn prompt_edit_before_first_prompt_starts_fresh_thread() -> Result<()> {
    let (mut app, mut app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let config = app.chat_widget.config_ref().clone();
    let source_thread_id = app_test_support::create_fake_rollout(
        config.codex_home.as_path(),
        "2025-01-05T12-00-00",
        "2025-01-05T12:00:00Z",
        "first prompt",
        Some("test-provider"),
        /*git_info*/ None,
    )
    .expect("materialized rollout should be created");
    let source_thread_id = ThreadId::from_string(&source_thread_id)?;
    std::fs::write(
        config.codex_home.join("config.toml"),
        "default_permissions = \":workspace\"\n[permissions.server-only]\nextends = \":read-only\"\n",
    )?;
    let server_config = ConfigBuilder::default()
        .codex_home(config.codex_home.to_path_buf())
        .build()
        .await?;
    let mut app_server =
        Box::pin(crate::start_embedded_app_server_for_picker(&server_config)).await?;
    let started = app_server
        .resume_thread(
            &crate::local_settings::LocalSettings::from(&config),
            config.clone(),
            source_thread_id,
            crate::app_server_session::ResumeModelSettings::OverrideFromCurrentConfig,
        )
        .await?;
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    app.select_permission_profile(
        &mut app_server,
        PermissionProfileSelection {
            profile_id: "server-only".into(),
            approval_policy: None,
            approvals_reviewer: None,
            display_label: "server-only".into(),
        },
    )
    .await;
    while app_event_rx.try_recv().is_ok() {}
    let mut tui = crate::tui::test_support::make_test_tui()?;
    Box::pin(app.handle_event(
        &mut tui,
        &mut app_server,
        AppEvent::ForkSessionForPromptEdit {
            thread_id: source_thread_id,
            nth_user_message: 0,
            prompt: crate::chatwidget::UserMessage::from("first prompt"),
        },
    ))
    .await?;
    assert_eq!(app.chat_widget.thread_id(), Some(source_thread_id));
    assert_eq!(app.chat_widget.composer_text_with_pending(), "first prompt");
    insta::assert_snapshot!(
        next_history_message(&mut app_event_rx),
        @"■ Wait for permissions to update before editing this prompt."
    );
    let settings = next_thread_settings_updated(&mut app_server, source_thread_id).await;
    app.enqueue_thread_notification(
        source_thread_id,
        ServerNotification::ThreadSettingsUpdated(settings),
    )
    .await?;
    while app_event_rx.try_recv().is_ok() {}
    let endpoint = crate::resolve_remote_addr("ws://127.0.0.1:8765")?;
    app.app_server_target = crate::AppServerTarget::Remote { endpoint };
    let local_only_root = tempdir()?;
    let local_only_root_path = local_only_root.path().abs();
    app.harness_overrides
        .additional_writable_roots
        .push(local_only_root.path().to_path_buf());
    app.refresh_in_memory_config_from_disk().await?;
    assert!(app.config.workspace_roots.contains(&local_only_root_path));

    let control = Box::pin(app.handle_event(
        &mut tui,
        &mut app_server,
        AppEvent::ForkSessionForPromptEdit {
            thread_id: source_thread_id,
            nth_user_message: 0,
            prompt: crate::chatwidget::UserMessage::from("first prompt"),
        },
    ))
    .await?;

    assert!(matches!(control, AppRunControl::Continue));
    let fresh_thread_id = app
        .chat_widget
        .thread_id()
        .expect("first prompt edit should start a fresh thread");
    assert_ne!(fresh_thread_id, source_thread_id);
    let active = app
        .chat_widget
        .config_ref()
        .permissions
        .active_permission_profile()
        .unwrap();
    assert_eq!(active.id, "server-only");
    let session = app
        .primary_session_configured
        .as_ref()
        .expect("new session");
    let roots = &session.runtime_workspace_roots;
    assert!(!roots.contains(&local_only_root_path));
    assert_eq!(app.chat_widget.composer_text_with_pending(), "first prompt");
    let history = std::iter::from_fn(|| app_event_rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::InsertHistoryCell(cell) => {
                Some(lines_to_single_string(&cell.display_lines(/*width*/ 120)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        history.iter().any(|line| {
            line == "• You’re continuing from this point in a new conversation"
        })
    );
    assert!(
        !history
            .iter()
            .any(|line| line.contains("Thread forked from"))
    );
    app_server.shutdown().await?;

    Ok(())
}
