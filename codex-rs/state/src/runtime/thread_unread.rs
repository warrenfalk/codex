use crate::StateRuntime;
use codex_protocol::ThreadId;
use sqlx::QueryBuilder;
use sqlx::Sqlite;
use std::collections::HashSet;

impl StateRuntime {
    /// Set acknowledgement state without changing thread recency or rollout metadata.
    pub async fn set_thread_unread(
        &self,
        thread_id: ThreadId,
        unread: bool,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query("UPDATE threads SET unread = ? WHERE id = ?")
            .bind(unread)
            .bind(thread_id.to_string())
            .execute(self.pool.as_ref())
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Read unread state for a bounded page of threads.
    pub async fn unread_thread_ids(
        &self,
        thread_ids: &[ThreadId],
    ) -> anyhow::Result<HashSet<ThreadId>> {
        if thread_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let mut query =
            QueryBuilder::<Sqlite>::new("SELECT id FROM threads WHERE unread = 1 AND id IN (");
        let mut ids = query.separated(", ");
        for id in thread_ids {
            ids.push_bind(id.to_string());
        }
        ids.push_unseparated(")");
        query
            .build_query_scalar::<String>()
            .fetch_all(self.pool.as_ref())
            .await?
            .into_iter()
            .map(|id| ThreadId::from_string(&id).map_err(Into::into))
            .collect()
    }
}
