use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_state::SqliteConfig;
use codex_state::StateRuntime;
use codex_state::ThreadMetadataBuilder;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn stale_rollout_upsert_preserves_manual_title() -> anyhow::Result<()> {
    let thread_id = ThreadId::new();
    let home = std::env::temp_dir().join(format!("codex-state-title-{thread_id}"));
    std::fs::create_dir_all(&home)?;
    let home = scopeguard::guard(home, |home| {
        let _ = std::fs::remove_dir_all(home);
    });
    let runtime = StateRuntime::init(
        SqliteConfig::new_for_testing(AbsolutePathBuf::try_from(home.as_path())?),
        "test-provider".to_string(),
    )
    .await?;
    let mut observed = ThreadMetadataBuilder::new(
        thread_id,
        home.join("rollout.jsonl"),
        Utc::now(),
        SessionSource::Cli,
    )
    .build("test-provider");
    observed.cwd = home.to_path_buf();
    observed.title = "First user message".to_string();
    observed.first_user_message = Some("\u{2003}First user message\u{2003}".to_string());
    runtime.upsert_thread(&observed).await?;

    // A rollout writer can read this row before a concurrent explicit rename.
    let stale = runtime.get_thread(thread_id).await?.expect("thread exists");
    runtime
        .update_thread_title(thread_id, "Manual thread name")
        .await?;
    let mut expected = runtime.get_thread(thread_id).await?.expect("thread exists");

    runtime.upsert_thread(&stale).await?;

    let actual = runtime.get_thread(thread_id).await?.expect("thread exists");
    // Upserts advance the update timestamp even when preserving the title.
    expected.updated_at = actual.updated_at;
    assert_eq!(actual, expected);

    // Explicit updates still work, and a derived title must remain refreshable.
    runtime.update_thread_title(thread_id, &stale.title).await?;
    let mut corrected = runtime.get_thread(thread_id).await?.expect("thread exists");
    corrected.title = "Corrected first message".to_string();
    corrected.first_user_message = Some(corrected.title.clone());
    runtime.upsert_thread(&corrected).await?;
    let actual = runtime.get_thread(thread_id).await?.expect("thread exists");
    corrected.updated_at = actual.updated_at;
    assert_eq!(actual, corrected);
    Ok(())
}
