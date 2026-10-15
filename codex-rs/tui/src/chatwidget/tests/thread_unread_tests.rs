use super::*;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

#[tokio::test]
async fn stopped_unread_footer_is_bright_blue_and_survives_an_empty_status_configuration() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.local_settings.tui.status_line = Some(Vec::new());
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
    let line = chat.bottom_pane.status_line_value().expect("unread footer");
    assert_eq!(line, Line::from(vec!["unread".light_blue()]));
    let buffer = terminal.backend().buffer();
    let foregrounds: Vec<_> = buffer
        .content
        .iter()
        .filter(|cell| cell.symbol() != " ")
        .filter(|cell| cell.fg == Color::LightBlue)
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert_eq!(foregrounds, vec!["u", "n", "r", "e", "a", "d"]);

    chat.bottom_pane.set_task_running(/*running*/ true);
    chat.set_unread_indicator(/*show_unread*/ true);
    assert_eq!(chat.bottom_pane.status_line_text(), None);
    chat.bottom_pane.set_task_running(/*running*/ false);
    chat.set_unread_indicator(/*show_unread*/ true);
    assert_eq!(chat.bottom_pane.status_line_text(), Some("unread".into()));
    chat.set_unread_indicator(/*show_unread*/ false);
    assert_eq!(chat.bottom_pane.status_line_text(), None);
}

#[tokio::test]
async fn unread_footer_precedes_custom_status_and_stays_visible_in_a_narrow_terminal() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.local_settings.tui.status_line = Some(vec!["run-state".into(), "model-name".into()]);
    chat.set_unread_indicator(/*show_unread*/ true);
    assert_eq!(
        chat.bottom_pane.status_line_text(),
        Some("unread · 5.6-Sol".into())
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

    chat.set_side_conversation_active(/*active*/ true);
    chat.set_unread_indicator(/*show_unread*/ true);
    assert!(
        !chat
            .bottom_pane
            .status_line_text()
            .unwrap()
            .contains("unread")
    );
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
