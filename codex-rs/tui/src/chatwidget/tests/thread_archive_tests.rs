use super::*;
use crate::app_event::ArchiveMode;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn slash_archive_selects_mode_and_preserves_command_recall() {
    for (command, expected_mode) in [
        ("/archive", ArchiveMode::NewChat),
        ("/archive new", ArchiveMode::NewChat),
        ("/archive exit", ArchiveMode::Exit),
        ("/archive   exit  ", ArchiveMode::Exit),
    ] {
        let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
        chat.bottom_pane
            .set_composer_text(command.to_string(), Vec::new(), Vec::new());
        chat.handle_key_event(KeyEvent::from(KeyCode::Enter));

        let modes = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|event| match event {
                AppEvent::ArchiveCurrentThread(mode) => Some(mode),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(modes, vec![expected_mode]);
        assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
        chat.bottom_pane
            .set_composer_text(String::new(), Vec::new(), Vec::new());
        chat.handle_key_event(KeyEvent::from(KeyCode::Up));
        assert_eq!(chat.bottom_pane.composer_text(), command.trim_end());
    }
}

#[tokio::test]
async fn slash_archive_is_disabled_while_task_running() {
    for args in ["", "new", "exit"] {
        let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
        chat.bottom_pane.set_task_running(/*running*/ true);

        chat.dispatch_command_with_args(SlashCommand::Archive, args.to_string(), Vec::new());

        let event = rx.try_recv().expect("expected disabled command error");
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                let rendered = lines_to_single_string(&cell.display_lines(/*width*/ 80));
                assert!(
                    rendered.contains("'/archive' is disabled while a task is in progress."),
                    "expected /archive task-running error, got {rendered:?}"
                );
            }
            other => panic!("expected InsertHistoryCell error, got {other:?}"),
        }
        assert_matches!(rx.try_recv(), Err(TryRecvError::Empty));
    }
}

#[tokio::test]
async fn slash_archive_is_unavailable_in_side_conversations() {
    for args in ["", "new", "exit"] {
        let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
        chat.active_side_conversation = true;

        chat.dispatch_command_with_args(SlashCommand::Archive, args.to_string(), Vec::new());

        let event = rx.try_recv().expect("expected side conversation error");
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                let rendered = lines_to_single_string(&cell.display_lines(/*width*/ 80));
                assert!(rendered.contains("'/archive' is unavailable in side conversations."));
            }
            other => panic!("expected InsertHistoryCell error, got {other:?}"),
        }
        assert_matches!(rx.try_recv(), Err(TryRecvError::Empty));
        assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
    }
}

#[tokio::test]
async fn slash_archive_invalid_argument_shows_usage_without_archiving() {
    for command in ["/archive done", "/archive exit new", "/archive --exit"] {
        let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
        chat.bottom_pane
            .set_composer_text(command.to_string(), Vec::new(), Vec::new());
        chat.handle_key_event(KeyEvent::from(KeyCode::Enter));

        let cells = drain_insert_history(&mut rx);
        let rendered = cells
            .iter()
            .map(|cell| lines_to_single_string(cell))
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("slash_archive_usage", rendered);
        assert_matches!(rx.try_recv(), Err(TryRecvError::Empty));
        assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
    }
}
