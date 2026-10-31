use super::*;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

#[tokio::test]
async fn stopped_unread_indicator_is_bright_blue_above_input_with_no_status_items() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.local_settings.tui.status_line = Some(Vec::new());
    chat.refresh_status_line();
    chat.set_unread_indicator(/*show_unread*/ true);
    let width = 60;
    let mut terminal = Terminal::new(TestBackend::new(width, chat.desired_height(width))).unwrap();
    terminal
        .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    assert_chatwidget_snapshot!(
        "stopped_unread_footer",
        normalized_backend_snapshot(terminal.backend())
    );
    let buffer = terminal.backend().buffer();
    let rows: Vec<_> = (0..buffer.area.height)
        .map(|row| {
            (0..width)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
        })
        .collect();
    let unread_row = rows
        .iter()
        .position(|row| row.starts_with("unread"))
        .expect("unread row");
    let input_row = rows
        .iter()
        .position(|row| row.contains('›'))
        .expect("input row");
    assert!(unread_row < input_row);
    let foregrounds: Vec<_> = (0..width)
        .map(|col| &buffer[(col, u16::try_from(unread_row).unwrap())])
        .filter(|cell| cell.symbol() != " ")
        .filter(|cell| cell.fg == Color::LightBlue)
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert_eq!(foregrounds, vec!["u", "n", "r", "e", "a", "d"]);

    chat.bottom_pane.set_task_running(/*running*/ true);
    chat.set_unread_indicator(/*show_unread*/ true);
    assert!(!render_bottom_popup(&chat, /*width*/ 60).contains("unread"));
    chat.bottom_pane.set_task_running(/*running*/ false);
    chat.set_unread_indicator(/*show_unread*/ true);
    assert!(render_bottom_popup(&chat, /*width*/ 60).contains("unread"));
    chat.set_unread_indicator(/*show_unread*/ false);
    assert!(!render_bottom_popup(&chat, /*width*/ 60).contains("unread"));
}

#[tokio::test]
async fn unread_indicator_shares_the_notes_row_and_stays_visible_in_a_narrow_terminal() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.local_settings.tui.status_line = Some(vec!["run-state".into(), "model-name".into()]);
    chat.refresh_status_line();
    chat.set_unread_indicator(/*show_unread*/ true);
    chat.record_note("note-1".into(), "Check the result".into());
    assert_eq!(
        chat.bottom_pane.status_line_text(),
        Some("Ready · 5.6-Sol".into())
    );
    let rendered = render_bottom_popup(&chat, /*width*/ 65);
    assert_chatwidget_snapshot!("stopped_unread_with_notes", rendered);
    assert!(
        rendered
            .lines()
            .any(|line| line == "unread · 1 note to self · /nts to view")
    );
    let width = 24;
    let mut terminal = Terminal::new(TestBackend::new(width, chat.desired_height(width))).unwrap();
    terminal
        .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    assert_chatwidget_snapshot!(
        "stopped_unread_footer_narrow",
        normalized_backend_snapshot(terminal.backend())
    );

    let buffer = terminal.backend().buffer();
    let unread_row = (0..buffer.area.height)
        .find(|&row| buffer[(0, row)].symbol() == "u")
        .expect("unread row");
    assert_eq!(
        (buffer[(0, unread_row)].fg, buffer[(9, unread_row)].fg),
        (Color::LightBlue, Color::Magenta),
    );

    chat.set_unread_indicator(/*show_unread*/ false);
    assert!(render_bottom_popup(&chat, /*width*/ 65).contains("1 note to self · /nts to view"));
    chat.set_unread_indicator(/*show_unread*/ true);
    chat.set_side_conversation_active(/*active*/ true);
    assert!(!render_bottom_popup(&chat, /*width*/ 65).contains("unread"));
}

#[tokio::test]
async fn acknowledgement_commands_reach_server_without_submitting_input_even_when_running() {
    for (command, unread) in [(SlashCommand::Ack, false), (SlashCommand::Unread, true)] {
        for running in [false, true] {
            let (mut chat, mut events, mut operations) =
                make_chatwidget_manual(/*model_override*/ None).await;
            let thread_id = ThreadId::from_u128(/*value*/ 42);
            chat.thread_id = Some(thread_id);
            chat.bottom_pane.set_task_running(running);
            chat.dispatch_command(command);
            let changes = std::iter::from_fn(|| events.try_recv().ok())
                .filter_map(|event| match event {
                    AppEvent::SetThreadUnread { thread_id, unread } => Some((thread_id, unread)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(changes, vec![(thread_id, unread)]);
            assert_matches!(operations.try_recv(), Err(TryRecvError::Empty));
        }
    }
}
