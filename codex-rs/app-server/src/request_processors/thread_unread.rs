use super::ThreadRequestProcessor;
use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::thread_unread::write_thread_unread;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadUnreadSetParams;
use codex_app_server_protocol::ThreadUnreadSetResponse;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_thread_store::ReadThreadParams;

impl ThreadRequestProcessor {
    pub(crate) async fn thread_unread_set(
        &self,
        params: ThreadUnreadSetParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let thread_id = ThreadId::from_string(&params.thread_id)
            .map_err(|err| invalid_request(format!("invalid thread id: {err}")))?;
        let mut thread_ids = if params.unread {
            vec![thread_id]
        } else {
            self.thread_manager
                .list_agent_subtree_thread_ids(thread_id)
                .await
                .map_err(|err| internal_error(format!("failed to check session activity: {err}")))?
        };
        thread_ids.sort_by_key(ToString::to_string);
        let mut guards = Vec::with_capacity(thread_ids.len());
        for id in &thread_ids {
            let state = self.thread_state_manager.thread_state(*id).await;
            let lock = state.lock().await.unread_lock.clone();
            guards.push(lock.lock_owned().await);
        }
        let mut active = false;
        for id in thread_ids {
            let watched_active = matches!(
                self.thread_watch_manager
                    .loaded_status_for_thread(&id.to_string())
                    .await,
                ThreadStatus::Active { .. }
            );
            let core_active = if let Ok(thread) = self.thread_manager.get_thread(id).await {
                matches!(thread.agent_status().await, AgentStatus::Running)
            } else {
                false
            };
            if watched_active || core_active {
                active = true;
                break;
            }
        }
        let unread = if !params.unread && active {
            self.thread_store
                .read_thread(ReadThreadParams {
                    thread_id,
                    include_archived: true,
                    include_history: false,
                })
                .await
                .map_err(|err| internal_error(format!("failed to read unread state: {err}")))?
                .unread
        } else {
            let change =
                write_thread_unread(&self.thread_manager, thread_id, params.unread).await?;
            let unread = change.unread;
            self.outgoing
                .send_server_notification(ServerNotification::ThreadUnreadChanged(change))
                .await;
            unread
        };
        Ok(Some(ThreadUnreadSetResponse { unread }.into()))
    }
}
