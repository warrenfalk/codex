use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn notes_reader_preserves_draft_and_running_turn_on_close() -> Result<()> {
    let mut app = make_test_app().await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    app.chat_widget.insert_str("Unsent draft");
    app.chat_widget.handle_server_notification(
        ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: ThreadId::new().to_string(),
            turn: Turn {
                id: "running".to_string(),
                items_view: codex_app_server_protocol::TurnItemsView::Full,
                items: Vec::new(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
            },
        }),
        /*replay_kind*/ None,
    );
    let before = app.chat_widget.capture_thread_input_state();
    app.open_notes(&mut tui);
    assert!(matches!(app.overlay, Some(Overlay::Notes(_))));
    app.handle_backtrack_overlay_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    )
    .await?;
    assert_eq!(app.chat_widget.capture_thread_input_state(), before);
    assert!(app.chat_widget.is_task_running_for_test());
    assert!(app.overlay.is_none());
    server.shutdown().await?;
    Ok(())
}
