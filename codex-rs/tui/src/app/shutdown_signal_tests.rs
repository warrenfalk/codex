use super::*;
use assert_matches::assert_matches;
use tokio::sync::mpsc;

impl ShutdownSignalReceiver for mpsc::UnboundedReceiver<()> {
    async fn recv(&mut self) -> Option<()> {
        mpsc::UnboundedReceiver::recv(self).await
    }
}

#[tokio::test]
async fn first_sigterm_requests_shutdown_first_exit() {
    let (app_tx, mut app_rx) = mpsc::unbounded_channel();
    let app_event_tx = AppEventSender::new(app_tx);
    let (signal_tx, signal_rx) = mpsc::unbounded_channel();
    let (force_tx, mut force_rx) = mpsc::unbounded_channel();

    let listener = tokio::spawn(run_shutdown_signal_listener(
        signal_rx,
        app_event_tx,
        move || {
            let _ = force_tx.send(());
        },
    ));

    signal_tx.send(()).expect("send first signal");

    assert_matches!(
        app_rx.recv().await,
        Some(AppEvent::Exit(ExitMode::ShutdownFirst))
    );
    assert!(
        force_rx.try_recv().is_err(),
        "first SIGTERM should not force immediate exit"
    );

    drop(signal_tx);
    listener.await.expect("listener exits");
}

#[tokio::test]
async fn second_sigterm_forces_immediate_exit() {
    let (app_tx, mut app_rx) = mpsc::unbounded_channel();
    let app_event_tx = AppEventSender::new(app_tx);
    let (signal_tx, signal_rx) = mpsc::unbounded_channel();
    let (force_tx, mut force_rx) = mpsc::unbounded_channel();

    let listener = tokio::spawn(run_shutdown_signal_listener(
        signal_rx,
        app_event_tx,
        move || {
            let _ = force_tx.send(());
        },
    ));

    signal_tx.send(()).expect("send first signal");
    assert_matches!(
        app_rx.recv().await,
        Some(AppEvent::Exit(ExitMode::ShutdownFirst))
    );

    signal_tx.send(()).expect("send second signal");
    assert_matches!(force_rx.recv().await, Some(()));

    drop(signal_tx);
    listener.await.expect("listener exits");
}
