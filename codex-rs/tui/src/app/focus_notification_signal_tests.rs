use super::*;
use assert_matches::assert_matches;
use tokio::sync::mpsc;

impl FocusNotificationSignalReceiver for mpsc::UnboundedReceiver<()> {
    async fn recv(&mut self) -> Option<()> {
        mpsc::UnboundedReceiver::recv(self).await
    }
}

#[tokio::test]
async fn each_sigurg_requests_a_focus_notification() {
    let (app_tx, mut app_rx) = mpsc::unbounded_channel();
    let app_event_tx = AppEventSender::new(app_tx);
    let (signal_tx, signal_rx) = mpsc::unbounded_channel();

    let listener = tokio::spawn(run_focus_notification_signal_listener(
        signal_rx,
        app_event_tx,
    ));

    signal_tx.send(()).expect("send first signal");
    assert_matches!(
        app_rx.recv().await,
        Some(AppEvent::FocusNotificationRequested)
    );

    signal_tx.send(()).expect("send second signal");
    assert_matches!(
        app_rx.recv().await,
        Some(AppEvent::FocusNotificationRequested)
    );

    drop(signal_tx);
    listener.await.expect("listener exits");
}
