use super::*;

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
