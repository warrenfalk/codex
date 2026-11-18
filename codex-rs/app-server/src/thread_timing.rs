//! Passive timing subscriptions. They never attach a client to an interactive thread.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadTimingActiveSpan;
use codex_app_server_protocol::ThreadTimingReadParams;
use codex_app_server_protocol::ThreadTimingReadResponse;
use codex_app_server_protocol::ThreadTimingSubscribeParams;
use codex_app_server_protocol::ThreadTimingSubscribeResponse;
use codex_app_server_protocol::ThreadTimingSubscriptionClosedNotification;
use codex_app_server_protocol::ThreadTimingUnsubscribeResponse;
use codex_app_server_protocol::ThreadTimingUpdatedNotification;
use codex_protocol::ThreadId;
use codex_thread_store::ReadTimingParams;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::error_code::method_not_found;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingMessageSender;

const MAX_SUBSCRIPTIONS: usize = 16;
const UPDATE_INTERVAL: Duration = Duration::from_millis(250);

#[cfg(test)]
#[path = "thread_timing_tests.rs"]
mod tests;

struct Subscription {
    stop: CancellationToken,
    done: oneshot::Receiver<()>,
}

pub(crate) struct ThreadTimingService {
    store: Arc<dyn ThreadStore>,
    outgoing: Arc<OutgoingMessageSender>,
    subscriptions: Mutex<HashMap<(ConnectionId, String), Subscription>>,
}

impl ThreadTimingService {
    pub(crate) fn new(store: Arc<dyn ThreadStore>, outgoing: Arc<OutgoingMessageSender>) -> Self {
        Self {
            store,
            outgoing,
            subscriptions: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn read(
        &self,
        params: ThreadTimingReadParams,
    ) -> Result<ThreadTimingReadResponse, JSONRPCErrorError> {
        read(self.store.as_ref(), params).await
    }

    pub(crate) async fn subscribe(
        &self,
        connection: ConnectionId,
        params: ThreadTimingSubscribeParams,
    ) -> Result<ThreadTimingSubscribeResponse, JSONRPCErrorError> {
        if params.subscription_id.is_empty() || params.subscription_id.len() > 128 {
            return Err(invalid_request(
                "subscriptionId must contain 1 to 128 bytes",
            ));
        }
        let read_params = ThreadTimingReadParams {
            thread_id: params.thread_id.clone(),
            turn_id: params.turn_id.clone(),
            cursor: params.cursor,
            limit: Some(500),
        };
        // Validate the source and cursor before acknowledging. Deliver this first page exactly
        // once in the same ordered stream as subsequent pages, avoiding a snapshot/subscribe gap.
        let first_page = self.read(read_params.clone()).await?;
        let key = (connection, params.subscription_id.clone());
        let mut entries = self.subscriptions.lock().await;
        entries.retain(|_, entry| !entry.stop.is_cancelled());
        if entries.contains_key(&key) {
            return Err(invalid_request(
                "subscriptionId already exists on this connection",
            ));
        }
        if entries.keys().filter(|(id, _)| *id == connection).count() >= MAX_SUBSCRIPTIONS {
            return Err(invalid_request(
                "too many timing subscriptions on this connection",
            ));
        }
        let stop = CancellationToken::new();
        let (done_tx, done) = oneshot::channel();
        entries.insert(
            key,
            Subscription {
                stop: stop.clone(),
                done,
            },
        );
        let outgoing = Arc::clone(&self.outgoing);
        let store = Arc::clone(&self.store);
        tokio::spawn(async move {
            let mut first = Some(first_page);
            let mut params_read = read_params;
            let mut last_cursor = params_read.cursor.clone();
            let mut had_active = false;
            let run = async {
                loop {
                    let page = match first.take() {
                        Some(page) => page,
                        None => match read(store.as_ref(), params_read.clone()).await {
                            Ok(page) => page,
                            Err(error) => {
                                outgoing
                                    .send_server_notification_to_connections(
                                        &[connection],
                                        ServerNotification::ThreadTimingSubscriptionClosed(
                                            ThreadTimingSubscriptionClosedNotification {
                                                subscription_id: params.subscription_id.clone(),
                                                thread_id: params.thread_id.clone(),
                                                error: error.message,
                                            },
                                        ),
                                    )
                                    .await;
                                break;
                            }
                        },
                    };
                    let more = page.next_cursor.is_some();
                    let changed = last_cursor.as_ref() != Some(&page.cursor)
                        || !page.data.is_empty()
                        || page.skipped_record_count > 0
                        || !page.active.is_empty()
                        || had_active;
                    had_active = !page.active.is_empty();
                    last_cursor = Some(page.cursor.clone());
                    params_read.cursor = Some(page.cursor.clone());
                    if changed {
                        outgoing
                            .send_server_notification_to_connections(
                                &[connection],
                                ServerNotification::ThreadTimingUpdated(
                                    ThreadTimingUpdatedNotification {
                                        subscription_id: params.subscription_id.clone(),
                                        thread_id: params.thread_id.clone(),
                                        turn_id: params.turn_id.clone(),
                                        page: page.into(),
                                    },
                                ),
                            )
                            .await;
                    }
                    if more {
                        tokio::task::yield_now().await;
                    } else {
                        tokio::time::sleep(UPDATE_INTERVAL).await;
                    }
                }
            };
            tokio::select! { biased; _ = stop.cancelled() => {}, _ = run => {} }
            stop.cancel();
            let _ = done_tx.send(());
        });
        Ok(ThreadTimingSubscribeResponse {})
    }

    pub(crate) async fn unsubscribe(
        &self,
        connection: ConnectionId,
        id: String,
    ) -> ThreadTimingUnsubscribeResponse {
        let entry = self.subscriptions.lock().await.remove(&(connection, id));
        if let Some(entry) = entry {
            entry.stop.cancel();
            let _ = entry.done.await;
        }
        ThreadTimingUnsubscribeResponse {}
    }

    pub(crate) async fn connection_closed(&self, connection: ConnectionId) {
        let entries = {
            let mut subscriptions = self.subscriptions.lock().await;
            subscriptions
                .extract_if(|(id, _), _| *id == connection)
                .map(|(_, subscription)| subscription)
                .collect::<Vec<_>>()
        };
        for entry in entries {
            entry.stop.cancel();
            let _ = entry.done.await;
        }
    }
}

impl Drop for ThreadTimingService {
    fn drop(&mut self) {
        for entry in self.subscriptions.get_mut().values() {
            entry.stop.cancel();
        }
    }
}

async fn read(
    store: &dyn ThreadStore,
    params: ThreadTimingReadParams,
) -> Result<ThreadTimingReadResponse, JSONRPCErrorError> {
    if params
        .turn_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 256)
    {
        return Err(invalid_request("turnId must contain 1 to 256 bytes"));
    }
    if params
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.len() > 16 * 1024)
    {
        return Err(invalid_request("timing cursor is too large"));
    }
    let thread_id = ThreadId::from_string(&params.thread_id)
        .map_err(|_| invalid_request("invalid thread id"))?;
    let page = store
        .read_timing(ReadTimingParams {
            thread_id,
            turn_id: params.turn_id.clone(),
            cursor: params.cursor,
            page_size: params.limit.unwrap_or(500) as usize,
        })
        .await
        .map_err(|error| match error {
            ThreadStoreError::InvalidRequest { message } => invalid_request(message),
            ThreadStoreError::ThreadNotFound { .. } => invalid_request("thread not found"),
            ThreadStoreError::Unsupported { operation } => {
                method_not_found(format!("{operation} is unavailable for this thread store"))
            }
            error => internal_error(error.to_string()),
        })?;
    Ok(ThreadTimingReadResponse {
        active: codex_rollout_trace::active_timing(thread_id, params.turn_id.as_deref())
            .into_iter()
            .map(|span| ThreadTimingActiveSpan {
                record: span.record.into(),
                elapsed_us: span.elapsed_us,
                stream: span.stream.map(Into::into),
            })
            .collect(),
        data: page.data,
        cursor: page.cursor,
        next_cursor: page.next_cursor,
        skipped_record_count: page.skipped_record_count,
        observed_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(i64::MAX),
    })
}
