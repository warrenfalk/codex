use std::io::Write;

use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_state::SqliteConfig;
use codex_state::StateRuntime;
use codex_state::ThreadMetadataBuilder;
use codex_thread_store::LocalThreadStore;
use codex_thread_store::LocalThreadStoreConfig;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadStore;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn legacy_index_updates_replace_only_fallback_titles()
-> Result<(), Box<dyn std::error::Error>> {
    for (source, title) in [
        (SessionSource::Cli, "First message"),
        (
            SessionSource::SubAgent(SubAgentSource::Other("guardian".to_string())),
            codex_state::GUARDIAN_THREAD_TITLE,
        ),
    ] {
        let home = TempDir::new()?;
        let thread_id = ThreadId::new();
        let directory = home.path().join("sessions/2025/01/03");
        std::fs::create_dir_all(&directory)?;
        let rollout = directory.join(format!("rollout-2025-01-03T12-00-00-{thread_id}.jsonl"));
        let mut file = std::fs::File::create(&rollout)?;
        for record in [
            json!({
                "timestamp": "2025-01-03T12:00:00Z", "type": "session_meta",
                "payload": {
                    "session_id": thread_id, "id": thread_id,
                    "timestamp": "2025-01-03T12:00:00Z", "cwd": home.path(),
                    "originator": "test", "cli_version": "test", "source": source,
                    "model_provider": "test-provider", "history_mode": "legacy"
                }
            }),
            json!({
                "timestamp": "2025-01-03T12:00:00Z", "type": "event_msg",
                "payload": {"type": "user_message", "message": "First message", "kind": "plain"}
            }),
        ] {
            writeln!(file, "{record}")?;
        }
        let sqlite = SqliteConfig::new_for_testing(AbsolutePathBuf::try_from(home.path())?);
        let runtime = StateRuntime::init(sqlite.clone(), "test-provider".to_string()).await?;
        let mut metadata = ThreadMetadataBuilder::new(thread_id, rollout, Utc::now(), source)
            .build("test-provider");
        metadata.cwd = home.path().to_path_buf();
        metadata.title = title.to_string();
        metadata.first_user_message = Some("First message".to_string());
        runtime.upsert_thread(&metadata).await?;
        let store = LocalThreadStore::new(
            LocalThreadStoreConfig {
                codex_home: home.path().to_path_buf(),
                sqlite,
                default_model_provider_id: "test-provider".to_string(),
            },
            Some(runtime.clone()),
        );

        for name in ["", "Latest manual name"] {
            codex_rollout::append_thread_name(home.path(), thread_id, name).await?;
            let thread = store
                .read_thread(ReadThreadParams {
                    thread_id,
                    include_archived: false,
                    include_history: false,
                })
                .await?;
            assert_eq!(thread.name, (!name.is_empty()).then(|| name.to_string()));
        }

        runtime
            .update_thread_title(thread_id, "New SQLite title")
            .await?;
        let thread = store
            .read_thread(ReadThreadParams {
                thread_id,
                include_archived: false,
                include_history: false,
            })
            .await?;
        assert_eq!(thread.name, Some("New SQLite title".to_string()));
    }
    Ok(())
}
