use super::*;
use codex_protocol::items::AgentMessageDelivery;
use codex_protocol::items::AsyncUserInputQuestion;
use pretty_assertions::assert_eq;

fn async_question_completed(message_id: &str, titles: &[&str]) -> ServerNotification {
    ServerNotification::ItemCompleted(ItemCompletedNotification {
        thread_id: "thread".into(),
        turn_id: "turn".into(),
        completed_at_ms: 0,
        item: AppServerThreadItem::AgentMessage {
            id: message_id.into(),
            text: titles.join("\n\n"),
            phase: Some(MessagePhase::FinalAnswer),
            memory_citation: None,
            delivery: Some(AgentMessageDelivery::Async),
            questions: Some(
                titles
                    .iter()
                    .map(|title| AsyncUserInputQuestion {
                        title: (*title).into(),
                        options: None,
                    })
                    .collect(),
            ),
        },
    })
}

#[tokio::test]
async fn async_question_notification_arrives_while_the_turn_keeps_running() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.on_task_started();

    chat.handle_server_notification(
        async_question_completed("question", &["Which interface?", "Any other details?"]),
        /*replay_kind*/ None,
    );

    assert!(chat.turn_lifecycle.agent_turn_running);
    assert!(!chat.bottom_pane.questions.as_ref().unwrap().expanded);
    let notification = chat
        .pending_notification
        .as_ref()
        .expect("question notification");
    insta::assert_snapshot!(notification.display(), @"Question: 2 questions requested");
}

#[tokio::test]
async fn async_question_notifications_do_not_repeat_for_handled_or_duplicate_items() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let event = async_question_completed("question", &["Which interface?"]);
    chat.handle_server_notification(event.clone(), /*replay_kind*/ None);
    chat.pending_notification
        .take()
        .expect("question notification");

    chat.handle_server_notification(event.clone(), /*replay_kind*/ None);
    assert!(chat.pending_notification.is_none());

    chat.bottom_pane.questions.as_mut().unwrap().accept_answer();
    let saved = chat.capture_thread_input_state();
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.restore_reconnected_input(saved, /*confirmed_message_ids*/ &[]);
    chat.handle_server_notification(event, /*replay_kind*/ None);
    assert!(chat.pending_notification.is_none());

    chat.handle_server_notification(
        async_question_completed("empty", &[]),
        /*replay_kind*/ None,
    );
    assert!(chat.pending_notification.is_none());

    chat.handle_server_notification(
        async_question_completed("new-question", &["Which interface?"]),
        /*replay_kind*/ None,
    );
    assert_eq!(
        chat.pending_notification
            .as_ref()
            .map(Notification::display),
        Some("Question: Which interface?".into()),
    );
}

#[tokio::test]
async fn async_question_notification_has_priority_over_completion_and_focus() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.handle_server_notification(
        async_question_completed("question", &["Which interface?"]),
        /*replay_kind*/ None,
    );
    chat.notify(Notification::AgentTurnComplete {
        response: "Done".into(),
    });
    chat.notify(Notification::FocusRequested);

    assert_eq!(
        chat.pending_notification
            .as_ref()
            .map(Notification::display),
        Some("Question: Which interface?".into()),
    );
}

#[test]
fn focus_requested_notification_uses_normal_filter_and_message() {
    let notification = Notification::FocusRequested;

    assert!(notification.allowed_for(&Notifications::Custom(vec!["focus-requested".to_string(),])));
    assert!(!notification.allowed_for(&Notifications::Custom(vec![
        "approval-requested".to_string(),
    ])));
    assert!(!notification.allowed_for(&Notifications::Enabled(false)));
    assert_chatwidget_snapshot!("focus_requested_notification", notification.display());
}

#[tokio::test]
async fn focus_requested_notification_does_not_replace_pending_interactive_notification() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.notify(Notification::PlanModePrompt {
        title: "Choose an option".to_string(),
    });

    chat.notify(Notification::FocusRequested);

    assert_matches!(
        chat.pending_notification,
        Some(Notification::PlanModePrompt { ref title }) if title == "Choose an option"
    );
}
