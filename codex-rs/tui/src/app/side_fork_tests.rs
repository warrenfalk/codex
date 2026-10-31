use super::*;
use crate::app::test_support::make_test_app;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn close_keep_names_fork_and_returns_to_parent() -> Result<()> {
    let mut app = make_test_app().await;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let parent = server.start_thread(&app.config).await?;
    let parent_id = parent.session.thread_id;
    let side = server.start_thread(&app.side_fork_config()).await?;
    let side_id = side.session.thread_id;
    app.ensure_thread_channel(parent_id)
        .store
        .lock()
        .await
        .set_session(parent.session, Vec::new());
    app.ensure_thread_channel(side_id)
        .store
        .lock()
        .await
        .set_session(side.session, Vec::new());
    app.primary_thread_id = Some(parent_id);
    app.active_thread_id = Some(side_id);
    app.side_threads
        .insert(side_id, SideThreadState::new(parent_id));
    server.side_conversation_threads.insert(side_id);
    app.sync_side_thread_ui();
    let mut parent_before = server
        .thread_read(parent_id, /*include_turns*/ false)
        .await?;

    Box::pin(app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::KeepSideForkAndReturn {
            thread_id: side_id,
            name: "Saved pilot".into(),
        },
    ))
    .await?;

    assert_eq!(app.active_thread_id, Some(parent_id));
    assert_eq!(app.primary_thread_id, Some(parent_id));
    assert!(!app.chat_widget.side_conversation_active());
    assert!(app.side_threads.is_empty());
    assert!(server.side_conversation_threads.is_empty());
    assert_eq!(
        server
            .thread_read(side_id, /*include_turns*/ false)
            .await?
            .name
            .as_deref(),
        Some("Saved pilot")
    );
    let parent_after = server
        .thread_read(parent_id, /*include_turns*/ false)
        .await?;
    // Creation timestamps settle asynchronously. Navigation must preserve all other parent
    // metadata and capabilities, regardless of which timestamp source the two reads observe.
    parent_before.created_at = parent_after.created_at;
    parent_before.updated_at = parent_after.updated_at;
    parent_before.recency_at = parent_after.recency_at;
    assert_eq!(parent_after, parent_before);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn keep_side_fork_preserves_thread_and_restores_normal_commands() -> Result<()> {
    let mut app = make_test_app().await;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let started = server.start_thread(&app.config).await?;
    let thread_id = started.session.thread_id;
    let parent_thread_id = ThreadId::new();
    server
        .thread_inject_items(thread_id, vec![App::side_boundary_prompt_item()])
        .await?;
    let store = app.ensure_thread_channel(thread_id).store.clone();
    store.lock().await.set_session(started.session, Vec::new());
    app.active_thread_id = Some(thread_id);
    app.side_threads
        .insert(thread_id, SideThreadState::new(parent_thread_id));
    server.side_conversation_threads.insert(thread_id);
    app.sync_side_thread_ui();
    assert!(app.chat_widget.side_conversation_active());

    app.submit_active_thread_op(
        &mut server,
        AppCommand::set_thread_name("Pilot graduated".into()),
    )
    .await?;

    assert_eq!(app.active_thread_id, Some(thread_id));
    assert_eq!(app.primary_thread_id, Some(thread_id));
    assert!(!app.chat_widget.side_conversation_active());
    assert!(app.side_threads.is_empty());
    assert!(server.side_conversation_threads.is_empty());
    assert_eq!(
        store
            .lock()
            .await
            .session
            .as_ref()
            .expect("session")
            .forked_from_id,
        Some(parent_thread_id)
    );
    let saved_thread = server
        .thread_read(thread_id, /*include_turns*/ false)
        .await?;
    assert_eq!(saved_thread.id, thread_id.to_string());
    assert_eq!(saved_thread.name.as_deref(), Some("Pilot graduated"));
    server.shutdown().await?;
    let saved = std::fs::read_to_string(saved_thread.path.expect("persisted fork path"))?;
    let messages: Vec<_> = saved
        .lines()
        .map(serde_json::from_str::<codex_history::RolloutItem>)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(|item| match item {
            codex_history::RolloutItem::ResponseItem(envelope) => Some(envelope.item),
            _ => None,
        })
        .collect();
    let ResponseItem::Message {
        content: boundary_content,
        ..
    } = App::side_boundary_prompt_item()
    else {
        panic!("expected boundary message");
    };
    let boundary_index = messages.iter()
        .position(|message| matches!(message, ResponseItem::Message { role, content, .. } if role == "user" && *content == boundary_content))
        .expect("original boundary preserved");
    assert!(boundary_index < messages.len() - 1);
    let ResponseItem::Message { role, content, .. } = messages.last().expect("saved transition")
    else {
        panic!("expected transition message");
    };
    assert_eq!(role, "developer");
    insta::assert_snapshot!(
        "kept_side_fork_transition",
        serde_json::to_string_pretty(content)?
    );
    let mut recovered = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let resumed = recovered
        .resume_thread(
            &app.local_settings,
            app.config.clone(),
            thread_id,
            crate::app_server_session::ResumeModelSettings::RestoreFromThread,
        )
        .await?;
    assert_eq!(resumed.session.thread_id, thread_id);
    recovered.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn keep_side_fork_retains_side_mode_when_transition_save_fails() -> Result<()> {
    let mut app = make_test_app().await;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let started = server.start_thread(&app.config).await?;
    let side_id = started.session.thread_id;
    let parent_id = ThreadId::new();
    server
        .thread_inject_items(side_id, vec![App::side_boundary_prompt_item()])
        .await?;
    server.shutdown().await?;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    // Reading a saved unloaded thread succeeds, but injecting into it requires attachment.
    assert!(
        !server
            .thread_read(side_id, /*include_turns*/ false)
            .await?
            .ephemeral
    );
    app.active_thread_id = Some(side_id);
    app.side_threads
        .insert(side_id, SideThreadState::new(parent_id));
    server.side_conversation_threads.insert(side_id);
    app.sync_side_thread_ui();

    assert!(
        !app.keep_side_conversation_as_fork(
            &mut server,
            "Keep me".into(),
            SideForkDestination::Continue
        )
        .await?
    );

    assert!(app.chat_widget.side_conversation_active());
    assert_eq!(app.active_side_parent_thread_id(), Some(parent_id));
    assert!(server.side_conversation_threads.contains(&side_id));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn discard_persisted_side_deletes_it_and_preserves_parent() -> Result<()> {
    let mut app = make_test_app().await;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let parent = server.start_thread(&app.config).await?;
    let side = server.start_thread(&app.side_fork_config()).await?;
    let parent_id = parent.session.thread_id;
    let side_id = side.session.thread_id;
    app.side_threads
        .insert(side_id, SideThreadState::new(parent_id));
    let parent_before = server
        .thread_read(parent_id, /*include_turns*/ false)
        .await?;

    assert!(app.discard_side_thread(&mut server, side_id).await);

    assert!(
        server
            .thread_read(side_id, /*include_turns*/ false)
            .await
            .is_err()
    );
    assert_eq!(
        server
            .thread_read(parent_id, /*include_turns*/ false)
            .await?,
        parent_before
    );
    assert!(app.side_threads.is_empty());
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn keep_side_fork_retains_side_mode_when_server_read_fails() -> Result<()> {
    let mut app = make_test_app().await;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let side_id = ThreadId::new();
    let parent_id = ThreadId::new();
    app.active_thread_id = Some(side_id);
    app.side_threads
        .insert(side_id, SideThreadState::new(parent_id));
    server.side_conversation_threads.insert(side_id);
    app.sync_side_thread_ui();

    let mut tui = crate::tui::test_support::make_test_tui()?;
    Box::pin(app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::KeepSideForkAndReturn {
            thread_id: side_id,
            name: "Keep me".into(),
        },
    ))
    .await?;

    assert!(app.chat_widget.side_conversation_active());
    assert_eq!(app.active_thread_id, Some(side_id));
    assert_eq!(app.active_side_parent_thread_id(), Some(parent_id));
    assert!(server.side_conversation_threads.contains(&side_id));
    server.shutdown().await?;
    Ok(())
}
