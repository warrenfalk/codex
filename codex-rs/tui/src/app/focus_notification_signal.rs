//! Unix signal bridge for requesting a focusable desktop notification.

use std::future::Future;

use tokio::signal::unix::Signal;
use tokio::signal::unix::SignalKind;
use tokio::task::JoinHandle;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

#[cfg(test)]
#[path = "focus_notification_signal_tests.rs"]
mod tests;

/// Source of external focus-notification requests consumed by the TUI signal bridge.
trait FocusNotificationSignalReceiver: Send + 'static {
    fn recv(&mut self) -> impl Future<Output = Option<()>> + Send;
}

impl FocusNotificationSignalReceiver for Signal {
    async fn recv(&mut self) -> Option<()> {
        Signal::recv(self).await
    }
}

pub(super) struct FocusNotificationSignalTask {
    handle: JoinHandle<()>,
}

impl FocusNotificationSignalTask {
    pub(super) fn spawn(app_event_tx: AppEventSender) -> Self {
        let handle = tokio::spawn(async move {
            let signals = match tokio::signal::unix::signal(SignalKind::from_raw(libc::SIGURG)) {
                Ok(signals) => signals,
                Err(err) => {
                    tracing::warn!(error = %err, "failed to install TUI SIGURG handler");
                    return;
                }
            };

            run_focus_notification_signal_listener(signals, app_event_tx).await;
        });

        Self { handle }
    }
}

impl Drop for FocusNotificationSignalTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn run_focus_notification_signal_listener<R>(mut signals: R, app_event_tx: AppEventSender)
where
    R: FocusNotificationSignalReceiver,
{
    while signals.recv().await.is_some() {
        tracing::info!("received SIGURG; requesting TUI focus notification");
        app_event_tx.send(AppEvent::FocusNotificationRequested);
    }
}
