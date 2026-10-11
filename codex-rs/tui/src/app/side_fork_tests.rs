use super::*;
use crate::app::test_support::make_test_app;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn keep_side_fork_preserves_thread_and_restores_normal_commands() -> Result<()> {
    let mut app = make_test_app().await;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let started = server.start_thread(&app.config).await?;
    let thread_id = started.session.thread_id;
    let parent_thread_id = ThreadId::new();
    let store = app.ensure_thread_channel(thread_id).store.clone();
    store.lock().await.set_session(started.session, Vec::new());
    app.active_thread_id = Some(thread_id);
    app.side_threads
        .insert(thread_id, SideThreadState::new(parent_thread_id));
    server.side_conversation_threads.insert(thread_id);
    app.sync_side_thread_ui();
    assert!(app.chat_widget.side_conversation_active());

    app.keep_side_conversation_as_fork(&mut server).await?;

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
    assert_eq!(
        server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?
            .id,
        thread_id.to_string()
    );
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

    app.keep_side_conversation_as_fork(&mut server).await?;

    assert!(app.chat_widget.side_conversation_active());
    assert_eq!(app.active_thread_id, Some(side_id));
    assert_eq!(app.active_side_parent_thread_id(), Some(parent_id));
    assert!(server.side_conversation_threads.contains(&side_id));
    server.shutdown().await?;
    Ok(())
}
