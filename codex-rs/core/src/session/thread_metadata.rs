use codex_thread_store::StoredThread;
use codex_thread_store::ThreadMetadataPatch;
use codex_thread_store::ThreadStoreError;
use codex_thread_store::ThreadStoreResult;

use super::session::Session;

impl Session {
    pub(crate) async fn update_thread_metadata(
        &self,
        patch: ThreadMetadataPatch,
        include_archived: bool,
    ) -> ThreadStoreResult<StoredThread> {
        let name_update = patch.name.clone();
        let _name_update_guard = if name_update.is_some() {
            Some(
                self.thread_name_update_lock
                    .acquire()
                    .await
                    .map_err(|err| ThreadStoreError::Internal {
                        message: format!("thread name update semaphore was closed: {err}"),
                    })?,
            )
        } else {
            None
        };
        let live_thread = self
            .live_thread_for_persistence("update thread metadata")
            .map_err(|err| ThreadStoreError::Internal {
                message: err.to_string(),
            })?;
        let updated = live_thread.update_metadata(patch, include_archived).await?;
        if let Some(name) = name_update {
            let mut state = self.state.lock().await;
            state.session_configuration.thread_name = name;
            state.disable_auto_thread_title();
        }
        Ok(updated)
    }
}
