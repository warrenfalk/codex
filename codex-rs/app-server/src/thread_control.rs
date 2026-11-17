//! Server-owned arbitration of external controllers and interactive input.
//!
//! Entry locks span command admission and submission, never the resulting agent turn.
//! Only lock markers persist; grants, request decisions, and timers belong to this process.

pub(crate) mod admission;
mod requests;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::outgoing_message::OutgoingMessageSender;
use crate::thread_state::ThreadStateManager;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadControlAction;
use codex_app_server_protocol::ThreadControlChangedNotification;
use codex_app_server_protocol::ThreadControlGrant;
use codex_app_server_protocol::ThreadControlReleaseParams;
use codex_app_server_protocol::ThreadControlRequest;
use codex_app_server_protocol::ThreadControlRequestParams;
use codex_app_server_protocol::ThreadControlRequestStatus;
use codex_app_server_protocol::ThreadControlState;
use codex_app_server_protocol::ThreadControlUpdateParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_core::ThreadManager;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadStore;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::OwnedMutexGuard;

pub(crate) const AUTO_GRANT_DELAY: std::time::Duration =
    std::time::Duration::from_secs(/*secs*/ 60);

#[derive(Default)]
pub(crate) struct ControlEntry {
    locked: bool,
    grant: Option<ThreadControlGrant>,
    request: Option<ThreadControlRequest>,
}

impl ControlEntry {
    fn grant(&mut self, controller_id: String) {
        self.grant = Some(ThreadControlGrant {
            id: uuid::Uuid::new_v4().to_string(),
            controller_id,
        });
        if let Some(request) = &mut self.request {
            request.status = ThreadControlRequestStatus::Granted;
            request.auto_grant_at = None;
        }
    }

    fn revoke(&mut self) -> bool {
        let changed = self.grant.take().is_some();
        if let Some(request) = &mut self.request
            && request.status == ThreadControlRequestStatus::Pending
        {
            request.status = ThreadControlRequestStatus::Denied;
            request.auto_grant_at = None;
            return true;
        }
        changed
    }
}

pub(crate) struct ThreadControlService {
    entries: Mutex<HashMap<ThreadId, Arc<Mutex<ControlEntry>>>>,
    lock_directory: PathBuf,
    threads: Arc<ThreadManager>,
    store: Arc<dyn ThreadStore>,
    thread_states: ThreadStateManager,
    outgoing: Arc<OutgoingMessageSender>,
}

impl ThreadControlService {
    pub(crate) fn new(
        codex_home: PathBuf,
        threads: Arc<ThreadManager>,
        store: Arc<dyn ThreadStore>,
        thread_states: ThreadStateManager,
        outgoing: Arc<OutgoingMessageSender>,
    ) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(HashMap::new()),
            lock_directory: codex_home.join("thread-control-locks"),
            threads,
            store,
            thread_states,
            outgoing,
        })
    }

    pub(crate) async fn entry(
        &self,
        id: &str,
    ) -> Result<(ThreadId, OwnedMutexGuard<ControlEntry>), JSONRPCErrorError> {
        let id = ThreadId::from_string(id)
            .map_err(|error| invalid_request(format!("invalid thread ID: {error}")))?;
        let existing = self.entries.lock().await.get(&id).cloned();
        let entry = if let Some(entry) = existing {
            entry
        } else {
            // Validate existence before allocating state for arbitrary caller-supplied IDs.
            if self.threads.get_thread(id).await.is_err() {
                self.store
                    .read_thread(ReadThreadParams {
                        thread_id: id,
                        include_archived: true,
                        include_history: false,
                    })
                    .await
                    .map_err(|error| invalid_request(format!("cannot read thread: {error}")))?;
            }
            let locked = match tokio::fs::metadata(self.lock_directory.join(id.to_string())).await {
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    return Err(internal_error(format!("cannot read thread lock: {error}")));
                }
            };
            Arc::clone(self.entries.lock().await.entry(id).or_insert_with(|| {
                Arc::new(Mutex::new(ControlEntry {
                    locked,
                    ..Default::default()
                }))
            }))
        };
        Ok((id, entry.lock_owned().await))
    }

    pub(crate) async fn snapshot(&self, id: ThreadId, entry: &ControlEntry) -> ThreadControlState {
        let interactive_attached = self.thread_states.has_interactive_subscribers(id).await;
        let state = self.thread_states.thread_state(id).await;
        let active_turn_id = state.lock().await.active_turn_id().map(str::to_owned);
        let turn_running = match self.threads.get_thread(id).await {
            Ok(thread) => matches!(thread.agent_status().await, AgentStatus::Running),
            Err(_) => false,
        } || active_turn_id.is_some();
        ThreadControlState {
            thread_id: id.to_string(),
            locked: entry.locked,
            grant: entry.grant.clone(),
            request: entry.request.clone(),
            interactive_attached,
            active_turn_id,
            turn_running,
        }
    }

    async fn persist_thread(&self, id: ThreadId) -> Result<(), JSONRPCErrorError> {
        // Keep an empty durable thread usable if the last interface detaches during a request.
        if let Ok(thread) = self.threads.get_thread(id).await {
            thread.ensure_rollout_materialized().await;
            thread.flush_rollout().await.map_err(|error| {
                internal_error(format!("cannot persist controlled thread: {error}"))
            })?;
        }
        Ok(())
    }

    pub(crate) async fn publish(&self, id: ThreadId, entry: &ControlEntry) -> ThreadControlState {
        let state = self.snapshot(id, entry).await;
        // Broadcast reaches interfaces even when the thread isn't their active view.
        self.outgoing
            .send_server_notification(ServerNotification::ThreadControlChanged(
                ThreadControlChangedNotification {
                    state: state.clone(),
                },
            ))
            .await;
        state
    }
}

pub(crate) fn validate_label(label: &str) -> Result<(), JSONRPCErrorError> {
    if label.is_empty() || label.len() > 128 || label.chars().any(char::is_control) {
        return Err(invalid_request(
            "identity/request IDs must be 1-128 bytes without control characters",
        ));
    }
    Ok(())
}
