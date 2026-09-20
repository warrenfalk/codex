use super::*;
use crate::notes::NoteToSelf;
use pretty_assertions::assert_eq;
use ratatui::style::Color;

#[tokio::test]
async fn notes_indicator_updates_during_a_turn_and_survives_its_completion() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.insert_str("Draft stays here");
    handle_turn_started(&mut chat, "turn-1");
    let notification = ServerNotification::ItemCompleted(ItemCompletedNotification {
        completed_at_ms: 0,
        thread_id: thread_id.to_string(),
        turn_id: "turn-1".to_string(),
        item: AppServerThreadItem::NoteToSelf {
            id: "note-1".to_string(),
            note: "Check the result".to_string(),
        },
    });
    chat.handle_server_notification(notification.clone(), /*replay_kind*/ None);
    chat.handle_server_notification(notification, /*replay_kind*/ None);
    assert_eq!(
        chat.notes.entries,
        vec![NoteToSelf {
            id: "note-1".to_string(),
            text: "Check the result".to_string(),
        }]
    );
    assert_chatwidget_snapshot!(
        "notes_indicator_running",
        render_bottom_popup(&chat, /*width*/ 65)
    );
    assert!(chat.bottom_pane.is_task_running());
    assert_no_submit_op(&mut op_rx);

    chat.on_task_complete(
        /*last_agent_message*/ None, /*duration_ms*/ None, /*from_replay*/ false,
    );
    assert_chatwidget_snapshot!(
        "notes_indicator_idle",
        render_bottom_popup(&chat, /*width*/ 65)
    );
    assert_chatwidget_snapshot!(
        "notes_indicator_narrow",
        render_bottom_popup(&chat, /*width*/ 32)
    );
    let area = Rect::new(
        /*x*/ 0,
        /*y*/ 0,
        /*width*/ 65,
        chat.desired_height(/*width*/ 65),
    );
    let mut buffer = Buffer::empty(area);
    chat.render(area, &mut buffer);
    let indicator_row = (0..area.height)
        .find(|&row| {
            (0..area.width)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
                .contains("1 note to self")
        })
        .expect("visible indicator");
    assert_eq!(buffer[(0, indicator_row)].fg, Color::Magenta);
    assert_eq!(chat.bottom_pane.composer_text(), "Draft stays here");
}

#[tokio::test]
async fn notes_reload_replaces_removed_history_and_ignores_old_load_results() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.reset_notes();
    let previous = chat.notes.generation;
    chat.record_note("removed".to_string(), "Removed note".to_string());

    chat.reset_notes();
    let current = chat.notes.generation;
    chat.finish_notes_load(
        previous,
        Ok(vec![NoteToSelf {
            id: "removed".to_string(),
            text: "Removed note".to_string(),
        }]),
    );
    chat.finish_notes_load(current, Ok(Vec::new()));

    assert_eq!(chat.notes.entries, Vec::new());
    assert!(!render_bottom_popup(&chat, /*width*/ 65).contains("note to self"));
}
