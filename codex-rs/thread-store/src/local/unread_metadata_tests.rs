use super::LocalThreadStore;
use super::test_support::test_config;
use super::test_support::write_session_file_with_history_mode;
use crate::ListThreadsParams;
use crate::ReadThreadParams;
use crate::SortDirection;
use crate::ThreadMetadataPatch;
use crate::ThreadSortKey;
use crate::ThreadStore;
use crate::UpdateThreadMetadataParams;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test]
async fn unread_survives_reconciliation_reopening_and_all_listing_paths()
-> Result<(), Box<dyn std::error::Error>> {
    for history_mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let home = TempDir::new()?;
        let config = test_config(home.path());
        let uuid = Uuid::new_v4();
        let thread_id = ThreadId::from_string(&uuid.to_string())?;
        let path = write_session_file_with_history_mode(
            home.path(),
            "2025-01-03T14-20-00",
            uuid,
            history_mode,
        )?;
        let original_rollout = std::fs::read(&path)?;
        for unread in [true, false, true] {
            let runtime = codex_state::StateRuntime::init(
                config.sqlite.clone(),
                config.default_model_provider_id.clone(),
            )
            .await?;
            let store = LocalThreadStore::new(config.clone(), Some(runtime.clone()));
            let updated = store
                .update_thread_metadata(UpdateThreadMetadataParams {
                    thread_id,
                    patch: ThreadMetadataPatch {
                        unread: Some(unread),
                        ..Default::default()
                    },
                    include_archived: false,
                })
                .await?
                .expect("updated thread");
            let stale_metadata = runtime
                .get_thread(thread_id)
                .await?
                .expect("saved metadata");
            runtime.upsert_thread(&stale_metadata).await?;
            codex_rollout::state_db::reconcile_rollout(
                Some(runtime.as_ref()),
                &path,
                &config.default_model_provider_id,
                /*builder*/ None,
                &[],
                /*archived_only*/ None,
                /*new_thread_memory_mode*/ None,
            )
            .await;
            for include_history in [false, true] {
                if include_history && history_mode == ThreadHistoryMode::Paginated {
                    continue;
                }
                let read = store
                    .read_thread(ReadThreadParams {
                        thread_id,
                        include_archived: false,
                        include_history,
                    })
                    .await?;
                assert_eq!((read.unread, read.recency_at), (unread, updated.recency_at));
            }
            for use_state_db_only in [false, true] {
                // The paginated fixture has no user items; live paginated listings are covered by RPC tests.
                if history_mode == ThreadHistoryMode::Paginated {
                    continue;
                }
                let page = store
                    .list_threads(ListThreadsParams {
                        page_size: 10,
                        cursor: None,
                        sort_key: ThreadSortKey::UpdatedAt,
                        sort_direction: SortDirection::Desc,
                        allowed_sources: Vec::new(),
                        model_providers: Some(Vec::new()),
                        cwd_filters: None,
                        section: None,
                        project_id: None,
                        archived: false,
                        search_term: None,
                        relation_filter: None,
                        use_state_db_only,
                    })
                    .await?;
                assert_eq!(
                    page.items
                        .iter()
                        .map(|thread| (thread.thread_id, thread.unread))
                        .collect::<Vec<_>>(),
                    vec![(thread_id, unread)],
                    "{history_mode:?}, state DB only: {use_state_db_only}"
                );
            }
            assert_eq!(std::fs::read(&path)?, original_rollout);
        }
    }
    Ok(())
}
