//! Cursor reads of the selected rollout lineage. Only newly scanned bytes are revisited live.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::PathBuf;

use codex_app_server_protocol::ThreadTimingEntry;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use serde::Deserialize;
use serde::Serialize;

use super::LocalThreadStore;
use super::live_writer;
use super::thread_rollout_resolver;
use crate::ReadTimingParams;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;
use crate::TimingPage;

const MAX_SCAN_BYTES: u64 = 4 * 1024 * 1024;

#[cfg(test)]
#[path = "timing_scan_tests.rs"]
mod tests;

#[derive(Clone)]
struct Segment {
    id: ThreadId,
    path: PathBuf,
    start_ordinal: Option<u64>,
    end_ordinal: Option<u64>,
    end_byte_offset: Option<u64>,
}

impl Segment {
    fn identity(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        // Legacy windows can reuse a thread ID in distinct filenames. Archive/compression
        // moves preserve the plain filename, so they keep entry identities and cursors stable.
        (
            self.id,
            codex_rollout::plain_rollout_path(&self.path).file_name(),
        )
            .hash(&mut hash);
        hash.finish()
    }
}

#[derive(Deserialize, Serialize)]
struct Cursor {
    version: u8,
    thread_id: ThreadId,
    turn_id: Option<String>,
    revision: u64,
    segment: usize,
    offset: u64,
}

pub(super) async fn read(
    store: &LocalThreadStore,
    params: ReadTimingParams,
) -> ThreadStoreResult<TimingPage> {
    if !(1..=1000).contains(&params.page_size) {
        return Err(invalid("timing limit must be between 1 and 1000"));
    }
    let segments = segments(store, params.thread_id).await?;
    let mut hash = DefaultHasher::new();
    for segment in &segments {
        (
            segment.identity(),
            segment.start_ordinal,
            segment.end_ordinal,
            segment.end_byte_offset,
        )
            .hash(&mut hash);
    }
    let revision = hash.finish();
    let cursor = match params.cursor.as_deref() {
        Some(value) => {
            let cursor: Cursor =
                serde_json::from_str(value).map_err(|_| invalid("invalid timing cursor"))?;
            if cursor.version != 1
                || cursor.thread_id != params.thread_id
                || cursor.turn_id != params.turn_id
            {
                return Err(invalid("timing cursor belongs to another thread or turn"));
            }
            if cursor.revision != revision {
                return Err(invalid("timing history changed; restart with no cursor"));
            }
            if cursor.segment >= segments.len() {
                return Err(invalid("invalid timing cursor segment"));
            }
            cursor
        }
        None => Cursor {
            version: 1,
            thread_id: params.thread_id,
            turn_id: params.turn_id.clone(),
            revision,
            segment: 0,
            offset: 0,
        },
    };
    let (stop, keep_running) = tokio::sync::oneshot::channel::<()>();
    let result = tokio::task::spawn_blocking(move || {
        scan(segments, cursor, params.page_size, || stop.is_closed())
    })
    .await
    .map_err(internal)?;
    drop(keep_running);
    result
}

async fn segments(
    store: &LocalThreadStore,
    thread_id: ThreadId,
) -> ThreadStoreResult<Vec<Segment>> {
    let resolved =
        thread_rollout_resolver::resolve_current_including_archived(store, thread_id).await?;
    let (id, path, mode) = match resolved {
        Some(resolved) => {
            let meta = codex_rollout::read_session_meta_line(&resolved.path)
                .await
                .map_err(internal)?;
            (resolved.rollout_id, resolved.path, meta.meta.history_mode)
        }
        None => {
            // An untouched live thread has a selected writer but no durable file yet.
            let (writer, id, mode) = live_writer::live_writer_parts(store, thread_id).await?;
            return Ok(vec![Segment {
                id,
                path: writer.rollout_path().to_path_buf(),
                start_ordinal: (mode == ThreadHistoryMode::Paginated).then_some(1),
                end_ordinal: None,
                end_byte_offset: None,
            }]);
        }
    };
    if mode == ThreadHistoryMode::Legacy {
        return Ok(vec![Segment {
            id,
            path,
            start_ordinal: None,
            end_ordinal: None,
            end_byte_offset: None,
        }]);
    }
    let lineage = store
        .resolve_rollout_lineage_for_history_page(thread_id)
        .await?;
    Ok(lineage
        .segments()
        .iter()
        .map(|segment| Segment {
            id: segment.rollout_id,
            path: segment.rollout_path.clone(),
            start_ordinal: Some(segment.start_ordinal()),
            end_ordinal: segment.end_ordinal(),
            end_byte_offset: segment.end.map(|end| end.end_byte_offset),
        })
        .collect())
}

fn scan(
    segments: Vec<Segment>,
    mut cursor: Cursor,
    limit: usize,
    is_cancelled: impl Fn() -> bool,
) -> ThreadStoreResult<TimingPage> {
    let mut data = Vec::new();
    let mut scanned = 0;
    let mut skipped_record_count: u32 = 0;
    loop {
        let segment = &segments[cursor.segment];
        // Lazy live writers have not created their file. Do not materialize it for a reader.
        let plain = codex_rollout::plain_rollout_path(&segment.path);
        if cursor.offset == 0 && !plain.exists() && !plain.with_extension("jsonl.zst").exists() {
            return page(data, cursor, /*has_more*/ false, skipped_record_count);
        }
        let mut file = codex_rollout::open_rollout_seekable_reader(&segment.path, &is_cancelled)
            .map_err(internal)?;
        let file_end = file.seek(SeekFrom::End(0)).map_err(internal)?;
        let end = segment.end_byte_offset.unwrap_or(file_end).min(file_end);
        if cursor.offset > end {
            return Err(invalid("timing history shrank; restart with no cursor"));
        }
        if cursor.offset > 0 {
            file.seek(SeekFrom::Start(cursor.offset - 1))
                .map_err(internal)?;
            let mut previous = [0];
            file.read_exact(&mut previous).map_err(internal)?;
            if previous != [b'\n'] {
                return Err(invalid("timing cursor is not at a record boundary"));
            }
        }
        file.seek(SeekFrom::Start(cursor.offset))
            .map_err(internal)?;
        let mut reader = BufReader::new(file.take(end - cursor.offset));
        let mut bytes = Vec::new();
        loop {
            if is_cancelled() {
                return Err(internal("timing read cancelled"));
            }
            bytes.clear();
            if reader.read_until(b'\n', &mut bytes).map_err(internal)? == 0
                || !bytes.ends_with(b"\n")
            {
                break;
            }
            let offset = cursor.offset;
            cursor.offset += bytes.len() as u64;
            scanned += bytes.len() as u64;
            let value = serde_json::from_slice::<serde_json::Value>(&bytes);
            match value {
                Ok(value)
                    if value.get("type").and_then(|v| v.as_str()) == Some("timing_record") =>
                {
                    let in_range = match segment.start_ordinal {
                        None => true,
                        Some(start) => value
                            .get("ordinal")
                            .and_then(serde_json::Value::as_u64)
                            .is_some_and(|ordinal| {
                                ordinal >= start
                                    && segment.end_ordinal.is_none_or(|end| ordinal < end)
                            }),
                    };
                    if in_range {
                        match codex_rollout::decode_rollout_line(value) {
                            Ok(line) => match line.item {
                                codex_rollout::RolloutItem::TimingRecord(record)
                                    if record.version == 1 =>
                                {
                                    if cursor
                                        .turn_id
                                        .as_ref()
                                        .is_none_or(|turn| *turn == record.turn_id)
                                    {
                                        data.push(ThreadTimingEntry {
                                            id: format!(
                                                "{}:{}:{offset}",
                                                segment.id,
                                                segment.identity()
                                            ),
                                            record: record.into(),
                                        });
                                    }
                                }
                                _ => skipped_record_count = skipped_record_count.saturating_add(1),
                            },
                            Err(_) => skipped_record_count = skipped_record_count.saturating_add(1),
                        }
                    }
                }
                Err(_) => skipped_record_count = skipped_record_count.saturating_add(1),
                Ok(_) => {}
            }
            if data.len() == limit || scanned >= MAX_SCAN_BYTES {
                let more = cursor.offset < end || cursor.segment + 1 < segments.len();
                return page(data, cursor, more, skipped_record_count);
            }
        }
        if cursor.segment + 1 == segments.len() {
            return page(data, cursor, /*has_more*/ false, skipped_record_count);
        }
        cursor.segment += 1;
        cursor.offset = 0;
    }
}

fn page(
    data: Vec<ThreadTimingEntry>,
    cursor: Cursor,
    has_more: bool,
    skipped_record_count: u32,
) -> ThreadStoreResult<TimingPage> {
    let cursor = serde_json::to_string(&cursor).map_err(internal)?;
    Ok(TimingPage {
        data,
        next_cursor: has_more.then(|| cursor.clone()),
        cursor,
        skipped_record_count,
    })
}

fn invalid(message: &str) -> ThreadStoreError {
    ThreadStoreError::InvalidRequest {
        message: message.to_string(),
    }
}

fn internal(error: impl std::fmt::Display) -> ThreadStoreError {
    ThreadStoreError::Internal {
        message: format!("timing read failed: {error}"),
    }
}
