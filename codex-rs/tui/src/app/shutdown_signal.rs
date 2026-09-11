//! Unix signal bridge for graceful TUI shutdown.

use std::future::Future;

use tokio::signal::unix::Signal;
use tokio::signal::unix::SignalKind;
use tokio::task::JoinHandle;

use crate::app_event::AppEvent;
use crate::app_event::ExitMode;
use crate::app_event_sender::AppEventSender;

#[cfg(test)]
#[path = "shutdown_signal_tests.rs"]
mod tests;

/// Source of external shutdown notifications consumed by the TUI signal bridge.
trait ShutdownSignalReceiver: Send + 'static {
    fn recv(&mut self) -> impl Future<Output = Option<()>> + Send;
}

impl ShutdownSignalReceiver for Signal {
    async fn recv(&mut self) -> Option<()> {
        Signal::recv(self).await
    }
}

pub(super) struct ShutdownSignalTask {
    handle: JoinHandle<()>,
}

impl ShutdownSignalTask {
    pub(super) fn spawn(app_event_tx: AppEventSender) -> Self {
        let handle = tokio::spawn(async move {
            let signals = match tokio::signal::unix::signal(SignalKind::terminate()) {
                Ok(signals) => signals,
                Err(err) => {
                    tracing::warn!(error = %err, "failed to install TUI SIGTERM handler");
                    return;
                }
            };

            run_shutdown_signal_listener(signals, app_event_tx, || {
                if let Err(err) = crate::tui::restore_after_exit() {
                    tracing::warn!(error = %err, "failed to restore terminal after second SIGTERM");
                }
                std::process::exit(128 + libc::SIGTERM);
            })
            .await;
        });

        Self { handle }
    }
}

impl Drop for ShutdownSignalTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn run_shutdown_signal_listener<R, F>(
    mut signals: R,
    app_event_tx: AppEventSender,
    force_exit: F,
) where
    R: ShutdownSignalReceiver,
    F: FnOnce() + Send + 'static,
{
    if signals.recv().await.is_none() {
        return;
    }

    tracing::info!("received SIGTERM; requesting graceful TUI shutdown");
    app_event_tx.send(AppEvent::Exit(ExitMode::ShutdownFirst));

    if signals.recv().await.is_some() {
        tracing::warn!("received second SIGTERM; forcing immediate TUI exit");
        force_exit();
    }
}
