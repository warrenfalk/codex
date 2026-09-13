//! Retained agents data shared by the dashboard and passive JSON output.
//!
//! Notifications update the cache immediately and are replayed over in-flight reads.
//! Rendering, connection management, and interactive session ownership live outside
//! this model.

mod listing;
mod refresh;

#[cfg(test)]
#[path = "agents_model_tests.rs"]
mod tests;

use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_protocol::ThreadId;
pub(crate) use refresh::AgentsRefresh;
pub(crate) use refresh::RefreshRequest;
use std::collections::HashMap;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct AgentsModel {
    /// An ID without metadata records discovery until a later read succeeds.
    pub(crate) threads: HashMap<ThreadId, Option<Thread>>,
    pub(crate) last_messages: HashMap<ThreadId, String>,
    pub(crate) initialized: bool,
    pub(crate) request_id: Option<Uuid>,
    pub(crate) refresh_pending: bool,
    pub(crate) refresh_thread_ids: HashSet<ThreadId>,
    refresh_notifications: HashMap<ThreadId, Vec<ServerNotification>>,
    removed: HashSet<ThreadId>,
    reconcile_archives: bool,
}

impl AgentsModel {
    pub(crate) fn reset_connection(&mut self) {
        self.reconcile_archives = !self.threads.is_empty();
        self.initialized = false;
        self.request_id = None;
        self.refresh_pending = false;
        self.refresh_notifications.clear();
        self.last_messages.clear();
        self.removed.clear();
    }

    pub(crate) fn observe(&mut self, notification: &ServerNotification) -> bool {
        let id = match notification {
            ServerNotification::ThreadStarted(event) if !event.thread.ephemeral => &event.thread.id,
            ServerNotification::ThreadArchived(event) => &event.thread_id,
            ServerNotification::ThreadDeleted(event) => &event.thread_id,
            ServerNotification::ThreadUnarchived(event) => &event.thread_id,
            ServerNotification::ThreadClosed(event) => &event.thread_id,
            ServerNotification::ThreadStatusChanged(event) => &event.thread_id,
            ServerNotification::ThreadNameUpdated(event) => &event.thread_id,
            ServerNotification::ThreadSettingsUpdated(event) => &event.thread_id,
            ServerNotification::ThreadReverted(event) => &event.thread_id,
            ServerNotification::TurnStarted(event) => &event.thread_id,
            ServerNotification::TurnCompleted(event) => &event.thread_id,
            ServerNotification::ItemStarted(event)
                if matches!(event.item, ThreadItem::UserMessage { .. }) =>
            {
                &event.thread_id
            }
            _ => return false,
        };
        let Ok(id) = ThreadId::from_string(id) else {
            return false;
        };
        match notification {
            ServerNotification::ThreadStarted(event) => {
                self.removed.remove(&id);
                let mut thread = event.thread.clone();
                thread.turns.clear();
                self.threads.insert(id, Some(thread));
            }
            ServerNotification::ThreadUnarchived(_) => {
                self.removed.remove(&id);
            }
            ServerNotification::ThreadArchived(_) | ServerNotification::ThreadDeleted(_) => {
                self.removed.insert(id);
                self.threads.remove(&id);
                self.last_messages.remove(&id);
                self.refresh_thread_ids.remove(&id);
            }
            _ if self.removed.contains(&id) => return false,
            ServerNotification::ThreadClosed(_) => {
                if let Some(thread) = self.threads.get_mut(&id).and_then(Option::as_mut) {
                    thread.status = ThreadStatus::NotLoaded;
                }
            }
            ServerNotification::ThreadStatusChanged(event) => {
                if let Some(thread) = self.threads.get_mut(&id).and_then(Option::as_mut) {
                    thread.status = event.status.clone();
                }
            }
            ServerNotification::ThreadNameUpdated(event) => {
                if let Some(thread) = self.threads.get_mut(&id).and_then(Option::as_mut) {
                    thread.name.clone_from(&event.thread_name);
                }
            }
            ServerNotification::ThreadSettingsUpdated(event) => {
                if let Some(thread) = self.threads.get_mut(&id).and_then(Option::as_mut) {
                    thread.cwd.clone_from(&event.thread_settings.cwd);
                    thread
                        .model_provider
                        .clone_from(&event.thread_settings.model_provider);
                }
            }
            ServerNotification::ThreadReverted(_) => {
                self.last_messages.remove(&id);
            }
            _ => {}
        }
        if !self.removed.contains(&id) {
            self.refresh_thread_ids.insert(id);
        }
        if self.request_id.is_some() {
            self.refresh_pending = true;
            let pending = self.refresh_notifications.entry(id).or_default();
            pending.retain(|previous| {
                std::mem::discriminant(previous) != std::mem::discriminant(notification)
            });
            pending.push(notification.clone());
        }
        true
    }

    pub(crate) fn begin_refresh(&mut self) -> Option<RefreshRequest> {
        if self.request_id.is_some() {
            self.refresh_pending = true;
            return None;
        }
        if !self.initialized {
            self.refresh_thread_ids.extend(self.threads.keys().copied());
        }
        if self.initialized && self.refresh_thread_ids.is_empty() {
            return None;
        }
        let id = Uuid::new_v4();
        self.request_id = Some(id);
        self.refresh_pending = false;
        Some(RefreshRequest {
            id,
            discover: !self.initialized,
            reconcile_archives: self.reconcile_archives,
            threads: self
                .refresh_thread_ids
                .drain()
                .map(|id| (id, self.threads.get(&id).cloned().flatten()))
                .collect(),
            known_ids: self
                .threads
                .iter()
                .filter_map(|(id, thread)| thread.is_some().then_some(*id))
                .chain(self.removed.iter().copied())
                .collect(),
        })
    }

    pub(crate) fn finish_refresh(
        &mut self,
        request_id: Uuid,
        result: anyhow::Result<AgentsRefresh>,
    ) -> anyhow::Result<()> {
        if self.request_id != Some(request_id) {
            return Ok(());
        }
        self.request_id = None;
        let result = result.and_then(|refresh| {
            self.initialized = refresh.recent_seed_complete;
            if self.initialized {
                self.reconcile_archives = false;
            }
            self.last_messages.extend(refresh.last_messages);
            for id in refresh.removed {
                self.removed.insert(id);
                self.threads.remove(&id);
                self.last_messages.remove(&id);
            }
            for (id, thread) in refresh.threads {
                if self.removed.contains(&id) {
                    continue;
                }
                if let Some(mut thread) = thread {
                    if thread.ephemeral {
                        self.threads.remove(&id);
                        self.last_messages.remove(&id);
                        continue;
                    }
                    thread.turns.clear();
                    self.threads.insert(id, Some(thread));
                } else {
                    self.threads.entry(id).or_default();
                }
            }
            refresh.error.map_or(Ok(()), Err)
        });
        // Notifications win over older reads. Replayed changes also stay dirty so
        // the next read can reconcile metadata that notifications do not include.
        for notifications in std::mem::take(&mut self.refresh_notifications).into_values() {
            for notification in notifications {
                self.observe(&notification);
            }
        }
        self.refresh_pending = !self.refresh_thread_ids.is_empty();
        result
    }
}
