use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::LocalThreadStore;
use super::test_support::test_config;
use super::test_support::write_session_file_with_history_mode;
use crate::ReadTimingParams;
use crate::ThreadStore;
use crate::ThreadStoreError;

fn timing(thread_id: ThreadId, ordinal: u64, turn: &str) -> serde_json::Value {
    json!({ "timestamp": "2025-01-03T17:30:00Z", "ordinal": ordinal, "type": "timing_record",
        "payload": { "version": 1, "thread_id": thread_id, "turn_id": turn, "request_id": "request",
            "attempt": 1, "span_id": format!("span-{ordinal}"), "parent_span_id": null,
            "phase": "model_request", "event": "started", "at_ms": 1000 + ordinal,
            "model": "test-model", "unrelated_secret": "must-not-escape" } })
}

fn append(path: &Path, value: &serde_json::Value) {
    writeln!(
        OpenOptions::new().append(true).open(path).unwrap(),
        "{value}"
    )
    .unwrap();
}

fn params(thread_id: ThreadId) -> ReadTimingParams {
    ReadTimingParams {
        thread_id,
        turn_id: Some("turn".to_string()),
        cursor: None,
        page_size: 1,
    }
}

#[tokio::test]
async fn timing_pages_are_scoped_incremental_and_preserve_partial_lines() {
    for mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let home = TempDir::new().unwrap();
        let id = ThreadId::new();
        let path = write_session_file_with_history_mode(
            home.path(),
            "2025-01-03T17-30-00",
            id.to_string().parse().unwrap(),
            mode,
        )
        .unwrap();
        let store = LocalThreadStore::new(test_config(home.path()), None);
        append(&path, &timing(id, 1, "other"));
        append(&path, &timing(id, 2, "turn"));
        append(&path, &timing(id, 3, "turn"));
        let first = store.read_timing(params(id)).await.unwrap();
        assert_eq!(first.data.len(), 1);
        assert!(first.next_cursor.is_some());
        assert_eq!(
            serde_json::to_value(&first.data[0].record).unwrap(),
            json!({
                "version": 1, "threadId": id.to_string(), "turnId": "turn", "requestId": "request",
                "attempt": 1, "spanId": "span-2", "parentSpanId": null, "phase": "modelRequest",
                "event": { "type": "started", "atMs": 1002, "model": "test-model" }
            })
        );
        let second = store
            .read_timing(ReadTimingParams {
                cursor: Some(first.cursor),
                ..params(id)
            })
            .await
            .unwrap();
        assert_eq!(second.data[0].record.span_id, "span-3");
        assert_eq!(second.next_cursor, None);
        let late = timing(id, 4, "turn").to_string();
        write!(
            OpenOptions::new().append(true).open(&path).unwrap(),
            "{late}"
        )
        .unwrap();
        let partial = store
            .read_timing(ReadTimingParams {
                cursor: Some(second.cursor.clone()),
                ..params(id)
            })
            .await
            .unwrap();
        assert!(partial.data.is_empty());
        assert_eq!(partial.cursor, second.cursor);
        writeln!(OpenOptions::new().append(true).open(&path).unwrap()).unwrap();
        let late = store
            .read_timing(ReadTimingParams {
                cursor: Some(partial.cursor),
                ..params(id)
            })
            .await
            .unwrap();
        assert_eq!(late.data[0].record.span_id, "span-4");
        let wrong_scope = store
            .read_timing(ReadTimingParams {
                cursor: Some(late.cursor),
                turn_id: None,
                ..params(id)
            })
            .await;
        assert!(matches!(
            wrong_scope,
            Err(ThreadStoreError::InvalidRequest { .. })
        ));
    }
}

#[tokio::test]
async fn timing_reads_archived_compressed_history_and_reports_bad_records() {
    let home = TempDir::new().unwrap();
    let id = ThreadId::new();
    let path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T17-30-00",
        id.to_string().parse().unwrap(),
        ThreadHistoryMode::Legacy,
    )
    .unwrap();
    append(&path, &timing(id, 1, "turn"));
    let mut unsupported = timing(id, 2, "turn");
    unsupported["payload"]["version"] = json!(2);
    append(&path, &unsupported);
    writeln!(
        OpenOptions::new().append(true).open(&path).unwrap(),
        "malformed"
    )
    .unwrap();
    let store = LocalThreadStore::new(test_config(home.path()), None);
    let original = store
        .read_timing(ReadTimingParams {
            page_size: 100,
            ..params(id)
        })
        .await
        .unwrap();
    let archive = home.path().join("archived_sessions");
    std::fs::create_dir_all(&archive).unwrap();
    let compressed = archive
        .join(path.file_name().unwrap())
        .with_extension("jsonl.zst");
    let encoded = zstd::stream::encode_all(std::fs::File::open(&path).unwrap(), 1).unwrap();
    std::fs::write(&compressed, encoded).unwrap();
    std::fs::remove_file(&path).unwrap();
    let page = store
        .read_timing(ReadTimingParams {
            page_size: 100,
            ..params(id)
        })
        .await
        .unwrap();
    assert_eq!(page.data, original.data);
    assert_eq!(page.cursor, original.cursor);
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.skipped_record_count, 2);
    assert_eq!(page.next_cursor, None);
    let eof = store
        .read_timing(ReadTimingParams {
            cursor: Some(page.cursor.clone()),
            ..params(id)
        })
        .await
        .unwrap();
    assert!(eof.data.is_empty());
    assert_eq!(eof.cursor, page.cursor);
}

#[tokio::test]
async fn changing_a_legacy_window_invalidates_its_old_cursor() {
    let home = TempDir::new().unwrap();
    let id = ThreadId::new();
    let path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T17-30-00",
        id.to_string().parse().unwrap(),
        ThreadHistoryMode::Legacy,
    )
    .unwrap();
    append(&path, &timing(id, 1, "turn"));
    let store = LocalThreadStore::new(test_config(home.path()), None);
    let first = store.read_timing(params(id)).await.unwrap();
    let replacement = path.with_file_name(format!("rollout-2025-01-03T18-30-00-{id}.jsonl"));
    std::fs::rename(path, replacement).unwrap();
    let old = store
        .read_timing(ReadTimingParams {
            cursor: Some(first.cursor),
            ..params(id)
        })
        .await;
    assert!(matches!(old, Err(ThreadStoreError::InvalidRequest { .. })));
    let replacement = store.read_timing(params(id)).await.unwrap();
    assert_ne!(replacement.data[0].id, first.data[0].id);
}

#[tokio::test]
async fn timing_backfill_has_a_byte_budget_even_when_no_records_match() {
    let home = TempDir::new().unwrap();
    let id = ThreadId::new();
    let path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T17-30-00",
        id.to_string().parse().unwrap(),
        ThreadHistoryMode::Legacy,
    )
    .unwrap();
    for _ in 0..5 {
        append(
            &path,
            &json!({"type":"irrelevant", "payload":"x".repeat(1024 * 1024)}),
        );
    }
    append(&path, &timing(id, 1, "turn"));
    let store = LocalThreadStore::new(test_config(home.path()), None);
    let first = store.read_timing(params(id)).await.unwrap();
    assert!(first.data.is_empty());
    assert!(first.next_cursor.is_some());
    let second = store
        .read_timing(ReadTimingParams {
            cursor: Some(first.cursor),
            ..params(id)
        })
        .await
        .unwrap();
    assert_eq!(second.data.len(), 1);
    assert_eq!(second.next_cursor, None);
}
