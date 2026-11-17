//! Stable acknowledgement views over the upstream revisioned read receipts.

use crate::error_code::internal_error;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadReadState;
use codex_app_server_protocol::ThreadReadStateChangedNotification;
use codex_app_server_protocol::ThreadUnreadChangedNotification;
use codex_core::CodexThread;
use codex_protocol::ThreadId;

pub(crate) fn receipt_notifications(
    thread_id: ThreadId,
    read_state: ThreadReadState,
) -> [ServerNotification; 2] {
    [
        ServerNotification::ThreadUnreadChanged(ThreadUnreadChangedNotification {
            thread_id: thread_id.to_string(),
            unread: read_state.first_unread.is_some(),
        }),
        ServerNotification::ThreadReadStateChanged(ThreadReadStateChangedNotification {
            thread_id: thread_id.to_string(),
            read_state,
        }),
    ]
}

/// Prompt acceptance and terminal publication use the same turn key, so completion
/// cannot re-arm an already acknowledged result or replace its unread position.
pub(crate) async fn mark_thread_input_unread(
    thread: &CodexThread,
    thread_id: ThreadId,
    turn_id: &str,
) -> Result<Option<ThreadReadState>, JSONRPCErrorError> {
    thread.ensure_rollout_materialized().await;
    thread.flush_rollout().await.map_err(|error| {
        internal_error(format!("failed to persist accepted thread input: {error}"))
    })?;
    let db = thread
        .state_db()
        .ok_or_else(|| internal_error("thread read state unavailable"))?;
    if !db
        .publish_thread_attention(thread_id, turn_id)
        .await
        .map_err(|error| internal_error(format!("failed to mark accepted input unread: {error}")))?
    {
        return Ok(None);
    }
    let state = db
        .thread_read_states(&[thread_id])
        .await
        .map_err(|error| internal_error(format!("failed to read accepted input receipt: {error}")))?
        .remove(&thread_id)
        .ok_or_else(|| internal_error("accepted input receipt unavailable"))?;
    Ok(Some(crate::request_processors::thread_read_state::to_api(
        state,
    )))
}
