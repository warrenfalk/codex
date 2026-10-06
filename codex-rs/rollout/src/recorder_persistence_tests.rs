use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

fn writer_state(home: &TempDir) -> RolloutWriterState {
    RolloutWriterState {
        writer: None,
        deferred_creation: true,
        pending_items: VecDeque::new(),
        pending_bytes: 0,
        meta: None,
        cwd: home.path().to_path_buf(),
        rollout_path: home.path().join("sessions/rollout.jsonl"),
        ordinal_state: RolloutOrdinalState::Legacy,
        materialize_requested: false,
        degraded: None,
    }
}

fn checkpoint(size: usize) -> RolloutItem {
    serde_json::from_value(json!({
        "type": "compacted",
        "payload": {
            "message": "",
            "replacement_history": [{
                "type": "compaction",
                "encrypted_content": "a".repeat(size),
            }],
            "window_number": 1,
        },
    }))
    .expect("checkpoint")
}

#[tokio::test]
async fn oversized_checkpoint_is_saved_in_order() -> std::io::Result<()> {
    let home = TempDir::new()?;
    let mut state = writer_state(&home);
    let before = checkpoint(/*size*/ 1);
    let large = checkpoint(MAX_PENDING_ROLLOUT_BYTES + 1);
    let after = checkpoint(/*size*/ 2);
    state.add_items(vec![before.clone()]).await?;
    state.add_items(vec![large.clone()]).await?;
    state.add_items(vec![after.clone()]).await?;
    state.flush().await?;

    let saved = fs::read_to_string(&state.rollout_path)?
        .lines()
        .map(crate::parse_rollout_line)
        .map(|line| line.map(|line| line.item))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        serde_json::to_value(saved)?,
        serde_json::to_value(vec![before, large, after])?,
    );
    Ok(())
}

#[tokio::test]
async fn failed_storage_bounds_backlog_and_recovers_in_order() -> std::io::Result<()> {
    for size in [1, MAX_PENDING_ROLLOUT_BYTES + 1] {
        let home = TempDir::new()?;
        let mut state = writer_state(&home);
        let blocker = home.path().join("sessions");
        fs::write(&blocker, "not a directory")?;
        let queued = checkpoint(size);
        state.add_items(vec![queued.clone()]).await?;
        assert!(state.persist().await.is_err());
        let bytes_before = state.pending_bytes;
        let next = checkpoint(MAX_PENDING_ROLLOUT_BYTES + 2);
        assert!(state.add_items(vec![next.clone()]).await.is_err());
        assert_eq!(
            (state.pending_items.len(), state.pending_bytes),
            (1, bytes_before)
        );

        fs::remove_file(blocker)?;
        state.add_items(vec![next.clone()]).await?;
        state.shutdown().await?;
        let saved = fs::read_to_string(&state.rollout_path)?
            .lines()
            .map(crate::parse_rollout_line)
            .map(|line| line.map(|line| line.item))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            serde_json::to_value(saved)?,
            serde_json::to_value(vec![queued, next])?
        );
        assert_eq!((state.pending_items.len(), state.pending_bytes), (0, 0));
    }
    Ok(())
}
