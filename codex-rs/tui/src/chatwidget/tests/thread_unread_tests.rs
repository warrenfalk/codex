use super::*;
use pretty_assertions::assert_eq;

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
