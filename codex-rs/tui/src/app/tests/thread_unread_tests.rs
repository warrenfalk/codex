use super::session_lifecycle_requests::recorded_params;
use super::session_lifecycle_requests::start_recording_remote_app_server;
use super::*;
use crate::app_server_session::TurnPermissionsOverride;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadActiveFlag;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadStatusChangedNotification;
use codex_app_server_protocol::ThreadUnreadChangedNotification;
use codex_app_server_protocol::UserInput;
use codex_config::types::TuiKeymap;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

async fn start_unread_session(app: &mut App, server: &mut AppServerSession) -> Result<ThreadId> {
    let started = server.start_thread(&app.config).await?;
    let thread_id = started.session.thread_id;
    app.active_thread_id = Some(thread_id);
    app.chat_widget.handle_thread_session_quiet(started.session);
    server.thread_unread_set(thread_id, /*unread*/ true).await?;
    let thread = server
        .thread_read(thread_id, /*include_turns*/ false)
        .await?;
    app.agents_overview.threads.insert(thread_id, Some(thread));
    app.sync_thread_unread_indicator();
    Ok(thread_id)
}

#[tokio::test]
async fn focus_acknowledgement_waits_a_full_second_and_is_cancelled_by_focus_loss() -> Result<()> {
    let mut app = make_test_app().await;
    let (mut server, requests, proxy) = start_recording_remote_app_server(&app.config).await?;
    let thread_id = start_unread_session(&mut app, &mut server).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;

    tokio::time::pause();
    app.handle_tui_event(&mut tui, &mut server, TuiEvent::FocusGained)
        .await?;
    tokio::time::advance(Duration::from_millis(/*millis*/ 900)).await;
    app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
        .await;
    app.handle_tui_event(&mut tui, &mut server, TuiEvent::FocusLost)
        .await?;
    app.handle_tui_event(&mut tui, &mut server, TuiEvent::FocusGained)
        .await?;
    tokio::time::advance(Duration::from_millis(/*millis*/ 999)).await;
    app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
        .await;
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        vec![serde_json::json!({"threadId": thread_id.to_string(), "unread": true})]
    );
    tokio::time::advance(Duration::from_millis(/*millis*/ 1)).await;
    tokio::time::resume();
    app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
        .await;
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        vec![
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": false}),
        ]
    );
    assert!(
        !server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .unread
    );

    // A later unread change while focus stays put must not reuse the consumed attempt.
    server.thread_unread_set(thread_id, /*unread*/ true).await?;
    app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
        .await;
    assert!(
        server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .unread
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn focus_acknowledgement_is_cancelled_by_thread_switch_overview_and_suspend() -> Result<()> {
    let mut app = make_test_app().await;
    let (mut server, requests, proxy) = start_recording_remote_app_server(&app.config).await?;
    let thread_id = start_unread_session(&mut app, &mut server).await?;
    let tui = crate::tui::test_support::make_test_tui()?;

    enum FocusInterruption {
        Switch,
        Overview,
        Suspend,
        Reconnect,
    }
    app.app_server_target = AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr("ws://127.0.0.1:5001")?,
    };
    for action in [
        FocusInterruption::Switch,
        FocusInterruption::Suspend,
        FocusInterruption::Reconnect,
        FocusInterruption::Overview,
    ] {
        app.reconnect.offline = false;
        app.active_thread_id = Some(thread_id);
        app.handle_unread_focus(&tui, &mut server, &TuiEvent::FocusGained)
            .await;
        assert!(app.pending_focus_ack.is_some());
        match action {
            FocusInterruption::Switch => {
                app.clear_active_thread().await;
                app.active_thread_id = Some(thread_id);
            }
            FocusInterruption::Overview => app.open_agents_overview(&server),
            FocusInterruption::Suspend => {
                app.handle_unread_focus(&tui, &mut server, &TuiEvent::Resume)
                    .await
            }
            FocusInterruption::Reconnect => {
                app.begin_reconnect();
            }
        }
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(/*secs*/ 2)).await;
        tokio::time::resume();
        app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
            .await;
    }
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        vec![serde_json::json!({"threadId": thread_id.to_string(), "unread": true})]
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn unread_shortcut_preserves_drafts_and_supports_runtime_remapping() -> Result<()> {
    let mut app = make_test_app().await;
    let (mut server, requests, proxy) = start_recording_remote_app_server(&app.config).await?;
    let thread_id = start_unread_session(&mut app, &mut server).await?;
    app.chat_widget.local_settings.tui.status_line = Some(Vec::new());
    app.chat_widget.refresh_status_line();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Paste("Draft to finish later".into()),
    )
    .await?;
    app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyCode::Char('!').into()),
    )
    .await?;
    // The composer buffers plain typing briefly to distinguish it from a paste burst.
    tokio::time::pause();
    tokio::time::advance(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay()).await;
    tokio::time::resume();
    app.chat_widget
        .handle_paste_burst_tick(tui.frame_requester());
    assert!(
        server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .unread
    );
    let draft = app.chat_widget.composer_text_with_pending();
    assert_eq!(draft, "Draft to finish later!");

    // A queued notification can leave the local flag stale. Toggle the server's stored flag.
    app.agents_overview
        .threads
        .get_mut(&thread_id)
        .unwrap()
        .as_mut()
        .unwrap()
        .unread = false;
    for unread in [false, true] {
        app.handle_tui_event(
            &mut tui,
            &mut server,
            TuiEvent::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)),
        )
        .await?;
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            app.handle_tui_event(
                &mut tui,
                &mut server,
                TuiEvent::Key(KeyEvent::new_with_kind(
                    KeyCode::Char('m'),
                    KeyModifiers::ALT,
                    kind,
                )),
            )
            .await?;
        }
        assert_eq!(
            (
                server
                    .thread_read(thread_id, /*include_turns*/ false)
                    .await?
                    .unread,
                app.chat_widget.composer_text_with_pending()
            ),
            (unread, draft.clone())
        );
    }
    insta::assert_snapshot!(
        "unread_shortcut_preserves_draft",
        render_bottom_popup(&app.chat_widget, /*width*/ 65)
    );

    for (configured, keys, unread) in [
        ("'f12'", vec![KeyCode::F(12).into()], false),
        (
            "'ctrl-x u'",
            vec![
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
                KeyCode::Char('u').into(),
            ],
            true,
        ),
        (
            "[]",
            vec![KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)],
            true,
        ),
    ] {
        let config: TuiKeymap = toml::from_str(&format!("[global]\ntoggle_unread = {configured}"))?;
        app.keymap = RuntimeKeymap::from_config(&config).expect("valid unread shortcut");
        app.chat_widget.apply_keymap_update(config, &app.keymap);
        for key in keys {
            app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key))
                .await?;
        }
        assert_eq!(
            (
                server
                    .thread_read(thread_id, /*include_turns*/ false)
                    .await?
                    .unread,
                app.chat_widget.composer_text_with_pending()
            ),
            (unread, draft.clone())
        );
    }
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        vec![
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": false}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": false}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
        ]
    );
    for method in ["turn/start", "turn/steer"] {
        assert_eq!(
            recorded_params(&requests, method),
            Vec::<serde_json::Value>::new()
        );
    }
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn explicitly_marking_unread_cancels_pending_focus_acknowledgement() -> Result<()> {
    let mut app = make_test_app().await;
    let (mut server, requests, proxy) = start_recording_remote_app_server(&app.config).await?;
    let thread_id = start_unread_session(&mut app, &mut server).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    for shortcut in [true, false] {
        app.set_thread_unread(&mut server, thread_id, /*unread*/ false)
            .await;
        app.handle_unread_focus(&tui, &mut server, &TuiEvent::FocusGained)
            .await;
        assert!(app.pending_focus_ack.is_some());
        if shortcut {
            app.handle_tui_event(
                &mut tui,
                &mut server,
                TuiEvent::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)),
            )
            .await?;
        } else {
            app.handle_event(
                &mut tui,
                &mut server,
                AppEvent::SetThreadUnread {
                    thread_id,
                    unread: true,
                },
            )
            .await?;
        }
        assert!(app.pending_focus_ack.is_none());
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(/*secs*/ 2)).await;
        tokio::time::resume();
        app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
            .await;
        assert!(
            server
                .thread_read(thread_id, /*include_turns*/ false)
                .await?
                .unread
        );
    }
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        vec![
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": false}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": false}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
        ]
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn unread_shortcut_has_no_effect_without_a_session_or_in_a_side_conversation() -> Result<()> {
    let mut app = make_test_app().await;
    let (mut server, requests, proxy) = start_recording_remote_app_server(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let toggle = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT);
    app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(toggle))
        .await?;
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        Vec::<serde_json::Value>::new()
    );
    let thread_id = start_unread_session(&mut app, &mut server).await?;
    app.chat_widget
        .set_side_conversation_active(/*active*/ true);
    app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(toggle))
        .await?;
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        vec![serde_json::json!({"threadId": thread_id.to_string(), "unread": true})]
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn ignored_acknowledgements_are_not_retried_after_work_stops() -> Result<()> {
    use codex_model_provider_info::ModelProviderInfo;
    use core_test_support::responses;
    use core_test_support::streaming_sse::StreamingSseChunk;
    use core_test_support::streaming_sse::start_streaming_sse_server;

    let (release, gate) = tokio::sync::oneshot::channel();
    let (model_server, _completions) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_response_created("focus-test")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![responses::ev_completed("focus-test")]),
        },
    ]])
    .await;
    let mut app = make_test_app().await;
    let provider_id = "unread-focus-test";
    let base_url = format!("{}/v1", model_server.uri());
    std::fs::write(
        app.config.codex_home.join("config.toml"),
        format!(
            r#"
model = "gpt-5.2"
model_provider = "{provider_id}"
[model_providers.{provider_id}]
name = "Unread focus test"
base_url = "{base_url}"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )?;
    app.config.model = Some("gpt-5.2".into());
    app.config.model_provider_id = provider_id.into();
    app.config.model_provider = ModelProviderInfo {
        name: "Unread focus test".into(),
        base_url: Some(base_url),
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        ..ModelProviderInfo::default()
    };
    let (mut server, requests, proxy) = start_recording_remote_app_server(&app.config).await?;
    let thread_id = start_unread_session(&mut app, &mut server).await?;
    server
        .turn_start(
            thread_id,
            "focus-test-prompt".into(),
            vec![UserInput::Text {
                text: "Wait until released.".into(),
                text_elements: Vec::new(),
            }],
            app.config.cwd.to_path_buf(),
            /*approval_policy*/ None,
            /*approvals_reviewer*/ None,
            TurnPermissionsOverride::Preserve,
            &app.config.workspace_roots,
            "gpt-5.2".into(),
            /*effort*/ None,
            /*summary*/ None,
            /*service_tier*/ None,
            /*collaboration_mode*/ None,
            /*output_schema*/ None,
            /*cyber_access_program*/ None,
        )
        .await?;
    model_server.wait_for_request_count(/*count*/ 1).await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tokio::time::pause();
    app.handle_unread_focus(&tui, &mut server, &TuiEvent::FocusGained)
        .await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
    tokio::time::resume();
    app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
        .await;
    assert!(
        server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .unread
    );
    app.track_agents_overview_notification(&ServerNotification::ThreadStatusChanged(
        ThreadStatusChangedNotification {
            thread_id: thread_id.to_string(),
            status: ThreadStatus::Active {
                active_flags: Vec::new(),
            },
        },
    ));
    assert!(!render_bottom_popup(&app.chat_widget, /*width*/ 65).contains("unread"));
    app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)),
    )
    .await?;
    assert!(
        server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .unread
    );
    release.send(()).expect("release completion");
    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
        loop {
            let thread = server
                .thread_read(thread_id, /*include_turns*/ false)
                .await?;
            if !matches!(thread.status, ThreadStatus::Active { .. }) {
                break Result::<_>::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    app.handle_unread_focus(&tui, &mut server, &TuiEvent::Draw)
        .await;
    assert_eq!(
        recorded_params(&requests, "thread/unread/set"),
        vec![
            serde_json::json!({"threadId": thread_id.to_string(), "unread": true}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": false}),
            serde_json::json!({"threadId": thread_id.to_string(), "unread": false}),
        ]
    );
    assert!(
        server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .unread
    );
    app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)),
    )
    .await?;
    assert!(
        !server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .unread
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn unread_indicator_tracks_notifications_and_active_descendants() -> Result<()> {
    let mut app = make_test_app().await;
    let root = ThreadId::new();
    let child = ThreadId::new();
    let grandchild = ThreadId::new();
    app.active_thread_id = Some(root);
    app.chat_widget.local_settings.tui.status_line = Some(Vec::new());
    app.chat_widget.refresh_status_line();
    for (id, parent) in [(root, None), (child, Some(root)), (grandchild, Some(child))] {
        let thread: Thread = serde_json::from_value(serde_json::json!({
            "id": id.to_string(), "sessionId": root.to_string(), "parentThreadId": parent,
            "preview": "Unread task", "ephemeral": false, "modelProvider": "openai",
            "createdAt": 1, "updatedAt": 2, "status": {"type": "idle"},
            "cwd": app.config.cwd, "cliVersion": "0.0.0", "source": "cli", "turns": []
        }))?;
        app.agents_overview.threads.insert(id, Some(thread));
    }
    app.track_agents_overview_notification(&ServerNotification::ThreadUnreadChanged(
        ThreadUnreadChangedNotification {
            thread_id: root.to_string(),
            unread: true,
        },
    ));
    assert!(render_bottom_popup(&app.chat_widget, /*width*/ 65).contains("unread"));
    for active_flags in [
        Vec::new(),
        vec![ThreadActiveFlag::WaitingOnApproval],
        vec![ThreadActiveFlag::WaitingOnUserInput],
    ] {
        app.track_agents_overview_notification(&ServerNotification::ThreadStatusChanged(
            ThreadStatusChangedNotification {
                thread_id: grandchild.to_string(),
                status: ThreadStatus::Active { active_flags },
            },
        ));
        assert!(!render_bottom_popup(&app.chat_widget, /*width*/ 65).contains("unread"));
    }
    app.track_agents_overview_notification(&ServerNotification::ThreadStatusChanged(
        ThreadStatusChangedNotification {
            thread_id: grandchild.to_string(),
            status: ThreadStatus::Idle,
        },
    ));
    assert!(render_bottom_popup(&app.chat_widget, /*width*/ 65).contains("unread"));
    app.active_thread_id = Some(child);
    app.sync_thread_unread_indicator();
    assert!(!render_bottom_popup(&app.chat_widget, /*width*/ 65).contains("unread"));
    app.active_thread_id = Some(root);
    app.track_agents_overview_notification(&ServerNotification::ThreadUnreadChanged(
        ThreadUnreadChangedNotification {
            thread_id: root.to_string(),
            unread: false,
        },
    ));
    assert!(!render_bottom_popup(&app.chat_widget, /*width*/ 65).contains("unread"));
    Ok(())
}
