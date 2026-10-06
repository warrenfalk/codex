//! Capacity waits are independent of transport retry budgets and never change models.

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::StreamErrorEvent;
use codex_protocol::protocol::WarningEvent;
use std::time::Duration;
use tokio::time::Instant;

const NOTIFY_AFTER: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(crate) struct CapacityRetryState {
    started_at: Option<Instant>,
    attempts: u32,
    notified: bool,
}

#[cfg(test)]
#[path = "capacity_retry_tests.rs"]
mod tests;

impl CapacityRetryState {
    /// The caller owns cancellation of this wait along with its request loop.
    pub(crate) async fn wait(&mut self, session: &Session, turn: &TurnContext, error: &CodexErr) {
        let now = Instant::now();
        let started_at = *self.started_at.get_or_insert(now);
        let delay =
            Duration::from_secs(5 * (1_u64 << self.attempts.min(4))).min(Duration::from_secs(60));
        self.attempts = self.attempts.saturating_add(1);
        let retry_at = error
            .retry_after()
            .map(codex_http_client::RetryAfter::deadline)
            .unwrap_or(now)
            .max(now + delay);
        session
            .send_event(
                turn,
                EventMsg::StreamError(StreamErrorEvent {
                    message: format!(
                        "Model at capacity. Retrying the same model in {}s...",
                        retry_at.saturating_duration_since(now).as_secs()
                    ),
                    codex_error_info: Some(CodexErrorInfo::ServerOverloaded),
                    additional_details: None,
                }),
            )
            .await;
        let notify_at = started_at + NOTIFY_AFTER;
        if !self.notified && notify_at <= retry_at {
            tokio::time::sleep_until(notify_at).await;
            session.send_event(turn, EventMsg::ModelCapacityWarning(WarningEvent {
                message: "The model has been at capacity for five minutes. Still retrying the same model; you can interrupt to choose another model.".to_string(),
            })).await;
            self.notified = true;
        }
        tokio::time::sleep_until(retry_at).await;
    }
}
