use pretty_assertions::assert_eq;
use tempfile::TempDir;
use uuid::Uuid;

use super::LocalThreadStore;
use super::test_support::test_config;
use super::test_support::write_session_file;
use crate::ThreadMetadataPatch;
use crate::ThreadStore;
use crate::UpdateThreadMetadataParams;

#[tokio::test]
async fn history_derived_title_preserves_explicit_legacy_name() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let uuid = Uuid::from_u128(/*v*/ 319);
    let thread_id =
        codex_protocol::ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
    write_session_file(home.path(), "2025-01-03T14-15-00", uuid).expect("session file");
    let runtime = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("state db should initialize");
    let store = LocalThreadStore::new(config, Some(runtime.clone()));
    store
        .update_thread_metadata(UpdateThreadMetadataParams {
            thread_id,
            patch: ThreadMetadataPatch {
                name: Some(Some("Manual thread name".to_string())),
                ..Default::default()
            },
            include_archived: false,
        })
        .await
        .expect("set thread name");
    let mut expected = runtime
        .get_thread(thread_id)
        .await
        .expect("read metadata")
        .expect("thread metadata");

    // An append-derived update may have been queued before the explicit rename.
    store
        .record_thread_metadata(UpdateThreadMetadataParams {
            thread_id,
            patch: ThreadMetadataPatch {
                title: expected.first_user_message.clone(),
                ..Default::default()
            },
            include_archived: false,
        })
        .await
        .expect("apply history-derived title");

    let actual = runtime
        .get_thread(thread_id)
        .await
        .expect("read metadata")
        .expect("thread metadata");
    // Metadata writes may advance the update timestamp.
    expected.updated_at = actual.updated_at;
    assert_eq!(actual, expected);
}
