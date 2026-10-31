use super::ThreadRequestProcessor;
use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadUnreadSetParams;
use codex_app_server_protocol::ThreadUnreadSetResponse;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadMetadataPatch;

impl ThreadRequestProcessor {
    pub(super) async fn lock_thread_acknowledgement(
        &self,
        thread_id: ThreadId,
    ) -> Result<(Vec<tokio::sync::OwnedMutexGuard<()>>, bool), JSONRPCErrorError> {
        let mut thread_ids = self
            .thread_manager
            .list_agent_subtree_thread_ids(thread_id)
            .await
            .map_err(|error| {
                internal_error(format!("failed to check session activity: {error}"))
            })?;
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
        Ok((guards, active))
    }

    pub(crate) async fn thread_unread_set(
        &self,
        params: ThreadUnreadSetParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let thread_id = ThreadId::from_string(&params.thread_id)
            .map_err(|error| invalid_request(format!("invalid thread id: {error}")))?;
        let (_guards, active) = self.lock_thread_acknowledgement(thread_id).await?;
        let unread = if !params.unread && active {
            self.thread_store
                .read_thread(ReadThreadParams {
                    thread_id,
                    include_archived: true,
                    include_history: false,
                })
                .await
                .map_err(|error| internal_error(format!("failed to read unread state: {error}")))?
                .unread
        } else {
            if let Ok(thread) = self.thread_manager.get_thread(thread_id).await {
                thread.ensure_rollout_materialized().await;
                thread.flush_rollout().await.map_err(|error| {
                    internal_error(format!(
                        "failed to persist thread before updating unread state: {error}"
                    ))
                })?;
            }
            self.thread_manager
                .update_thread_metadata(
                    thread_id,
                    ThreadMetadataPatch {
                        unread: Some(params.unread),
                        ..Default::default()
                    },
                    /*include_archived*/ true,
                )
                .await
                .map_err(|error| {
                    internal_error(format!("failed to update unread state: {error}"))
                })?;
            let db = self
                .state_db
                .as_ref()
                .ok_or_else(|| internal_error("thread read state unavailable"))?;
            let state = db
                .thread_read_states(&[thread_id])
                .await
                .map_err(|error| {
                    internal_error(format!("failed to read updated receipt: {error}"))
                })?
                .remove(&thread_id)
                .ok_or_else(|| internal_error("updated receipt unavailable"))?;
            let unread = state.first_unread_turn.is_some();
            for notification in crate::thread_unread::receipt_notifications(
                thread_id,
                super::thread_read_state::to_api(state),
            ) {
                self.outgoing.send_server_notification(notification).await;
            }
            unread
        };
        Ok(Some(ThreadUnreadSetResponse { unread }.into()))
    }
}
