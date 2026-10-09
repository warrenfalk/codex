use super::*;
use crate::app_event::ArchiveMode;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn archive_current_session_transitions_only_after_archiving() -> Result<()> {
    for mode in [ArchiveMode::NewChat, ArchiveMode::Exit] {
        let (mut app, _codex_home) = make_history_test_app().await?;
        let thread_id =
            create_history_rollout(&app.config, ThreadHistoryMode::Legacy, "archive me")?;
        let mut app_server = crate::start_embedded_app_server_for_picker(&app.config).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;

        let missing_thread_id = ThreadId::new();
        app.active_thread_id = Some(missing_thread_id);
        assert_matches!(
            Box::pin(app.handle_event(
                &mut tui,
                &mut app_server,
                AppEvent::ArchiveCurrentThread(mode),
            ))
            .await?,
            AppRunControl::Continue
        );
        assert_eq!(app.active_thread_id, Some(missing_thread_id));

        app.active_thread_id = Some(thread_id);
        let control = Box::pin(app.handle_event(
            &mut tui,
            &mut app_server,
            AppEvent::ArchiveCurrentThread(mode),
        ))
        .await?;
        match mode {
            ArchiveMode::NewChat => {
                assert_matches!(control, AppRunControl::Continue);
                let fresh_thread_id = app
                    .chat_widget
                    .thread_id()
                    .expect("successful archive should start a fresh thread");
                assert_ne!(fresh_thread_id, thread_id);
                assert_eq!(app.active_thread_id, Some(fresh_thread_id));
            }
            ArchiveMode::Exit => {
                assert_matches!(
                    control,
                    AppRunControl::Exit(ExitReason::Archived(id)) if id == thread_id
                );
                assert_eq!(app.active_thread_id, None);
                let output = app
                    .exit_info(ExitReason::Archived(thread_id))
                    .format_exit_messages(/*color_enabled*/ false)
                    .join("\n")
                    .replace(&thread_id.to_string(), "THREAD_ID");
                insta::assert_snapshot!("archive_exit", output);
            }
        }

        app_server.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn archive_current_thread_respects_mode_on_shared_servers() -> Result<()> {
    let endpoint = crate::resolve_remote_addr("ws://127.0.0.1:4500")?;
    for target in [
        AppServerTarget::LocalDaemon {
            allow_embedded_fallback: true,
            endpoint: endpoint.clone(),
        },
        AppServerTarget::Remote { endpoint },
    ] {
        for mode in [ArchiveMode::NewChat, ArchiveMode::Exit] {
            let (mut app, _codex_home) = make_history_test_app().await?;
            let thread_id =
                create_history_rollout(&app.config, ThreadHistoryMode::Legacy, "archive me")?;
            let (mut server, requests, proxy) = start_recording_app_server(
                &app.config,
                /*blocked_thread_list*/ None,
                /*failed_thread_name*/ None,
            )
            .await?;
            let resumed = server
                .resume_thread(
                    &app.local_settings,
                    app.config.clone(),
                    thread_id,
                    crate::app_server_session::ResumeModelSettings::RestoreFromThread,
                )
                .await?;
            let mut side_config = app.config.clone();
            side_config.ephemeral = true;
            let side = server
                .fork_side_thread(&app.local_settings, side_config, thread_id)
                .await?;
            let side_id = side.session.thread_id;
            app.side_threads
                .insert(side_id, SideThreadState::new(thread_id));
            app.app_server_target = target.clone();
            app.enqueue_primary_thread_session(resumed.session.clone(), resumed.turns)
                .await?;
            app.chat_widget.handle_thread_session(resumed.session);
            app.chat_widget.insert_str("Unsent archived draft");
            let mut tui = crate::tui::test_support::make_test_tui()?;
            let (tx, _events) = tokio::sync::mpsc::unbounded_channel();
            app.app_event_tx = AppEventSender::new(tx);
            requests.lock().expect("request recorder lock").clear();

            let missing_thread_id = ThreadId::new();
            app.active_thread_id = Some(missing_thread_id);
            assert_matches!(
                Box::pin(app.handle_event(
                    &mut tui,
                    &mut server,
                    AppEvent::ArchiveCurrentThread(mode),
                ))
                .await?,
                AppRunControl::Continue
            );
            assert_eq!(
                (app.active_thread_id, app.chat_widget.thread_id()),
                (Some(missing_thread_id), Some(thread_id))
            );
            assert_eq!(
                recorded_params(&requests, "thread/start"),
                Vec::<serde_json::Value>::new()
            );
            assert_eq!(
                recorded_params(&requests, "thread/unsubscribe"),
                vec![serde_json::json!({"threadId": side_id.to_string()})]
            );
            requests.lock().expect("request recorder lock").clear();

            app.active_thread_id = Some(thread_id);
            let control = Box::pin(app.handle_event(
                &mut tui,
                &mut server,
                AppEvent::ArchiveCurrentThread(mode),
            ))
            .await?;
            match mode {
                ArchiveMode::NewChat => {
                    assert_matches!(control, AppRunControl::Continue);
                    let fresh_thread_id = app.chat_widget.thread_id().expect("fresh thread");
                    assert_ne!(fresh_thread_id, thread_id);
                    assert_eq!(
                        (
                            app.active_thread_id,
                            app.primary_thread_id,
                            app.chat_widget.thread_id()
                        ),
                        (
                            Some(fresh_thread_id),
                            Some(fresh_thread_id),
                            Some(fresh_thread_id)
                        )
                    );
                    assert_eq!(
                        app.thread_event_channels
                            .keys()
                            .copied()
                            .collect::<Vec<_>>(),
                        vec![fresh_thread_id]
                    );
                    assert!(app.chat_widget.composer_is_empty());
                }
                ArchiveMode::Exit => {
                    assert_matches!(
                        control,
                        AppRunControl::Exit(ExitReason::Archived(id)) if id == thread_id
                    );
                    assert_eq!(
                        (app.active_thread_id, app.chat_widget.thread_id()),
                        (None, Some(thread_id))
                    );
                    assert_eq!(
                        recorded_params(&requests, "thread/start"),
                        Vec::<serde_json::Value>::new()
                    );
                    assert!(app.thread_event_channels.is_empty());
                    assert_eq!(
                        app.exit_info(ExitReason::Archived(thread_id))
                            .format_exit_messages(/*color_enabled*/ false),
                        vec![format!("Session archived: {thread_id}")]
                    );
                }
            }
            assert!(!app.agents_overview.threads.contains_key(&thread_id));
            assert!(app.side_threads.is_empty());
            assert_eq!(
                recorded_params(&requests, "thread/archive"),
                vec![serde_json::json!({"threadId": thread_id.to_string()})]
            );
            // The failed archive already closed the side conversation before the retry.
            assert_eq!(
                recorded_params(&requests, "thread/unsubscribe"),
                Vec::<serde_json::Value>::new()
            );
            server.shutdown().await?;
            proxy.await??;
        }
    }
    Ok(())
}
