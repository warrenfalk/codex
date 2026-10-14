use crate::error_code::internal_error;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadUnreadChangedNotification;
use codex_core::ThreadManager;
use codex_protocol::ThreadId;
use codex_thread_store::ThreadMetadataPatch;

/// Persist acknowledgement independently of the rollout and return its observable update.
/// Callers serialize mutations with the thread's unread lock.
pub(crate) async fn write_thread_unread(
    thread_manager: &ThreadManager,
    thread_id: ThreadId,
    unread: bool,
) -> Result<ThreadUnreadChangedNotification, JSONRPCErrorError> {
    if let Ok(thread) = thread_manager.get_thread(thread_id).await {
        thread.ensure_rollout_materialized().await;
        thread.flush_rollout().await.map_err(|err| {
            internal_error(format!(
                "failed to persist thread before updating unread state: {err}"
            ))
        })?;
    }
    let thread = thread_manager
        .update_thread_metadata(
            thread_id,
            ThreadMetadataPatch {
                unread: Some(unread),
                ..Default::default()
            },
            /*include_archived*/ true,
        )
        .await
        .map_err(|err| internal_error(format!("failed to update unread state: {err}")))?;
    Ok(ThreadUnreadChangedNotification {
        thread_id: thread_id.to_string(),
        unread: thread.unread,
    })
}
