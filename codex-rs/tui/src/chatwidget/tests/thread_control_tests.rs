use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn queued_user_submission_revokes_external_control_before_delivery() {
    let (mut chat, mut events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let id = ThreadId::new();
    chat.thread_id = Some(id);
    chat.on_task_started();
    while events.try_recv().is_ok() {}
    chat.handle_composer_input_result(
        InputResult::Queued {
            text: "My next instruction".to_owned(),
            text_elements: Vec::new(),
            action: QueuedInputAction::Plain,
            pending_pastes: Vec::new(),
        },
        /*had_modal_or_popup*/ false,
    );
    let mut revoked = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::ThreadControlUserInput { thread_id } = event {
            revoked.push(thread_id);
        }
    }
    assert_eq!(revoked, vec![id]);
    assert!(chat.turn_lifecycle.agent_turn_running);
}

#[tokio::test]
async fn control_command_remains_available_during_a_turn_without_revoking_its_own_action() {
    let (mut chat, mut events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let id = ThreadId::new();
    chat.thread_id = Some(id);
    chat.on_task_started();
    while events.try_recv().is_ok() {}
    chat.dispatch_command_with_args(SlashCommand::Control, "grant dot".to_owned(), Vec::new());
    let mut commands = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event {
            AppEvent::ThreadControlCommand { thread_id, command } => {
                commands.push((thread_id, command))
            }
            AppEvent::ThreadControlUserInput { .. } => {
                panic!("a control command must not revoke its own grant")
            }
            _ => {}
        }
    }
    assert_eq!(commands, vec![(id, "grant dot".to_owned())]);
    assert!(chat.turn_lifecycle.agent_turn_running);
}
