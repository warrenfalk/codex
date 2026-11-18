use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn inherited_timing_stops_at_the_frozen_prefix_and_then_pages_the_child() {
    let home = TempDir::new().unwrap();
    let parent_id = ThreadId::new();
    let child_id = ThreadId::new();
    let parent = home.path().join("parent.jsonl");
    let child = home.path().join("child.jsonl");
    let record = |thread: ThreadId, ordinal: u64| {
        json!({
            "timestamp":"2025-01-03T17:30:00Z", "ordinal":ordinal, "type":"timing_record", "payload":{
                "version":1,"thread_id":thread,"turn_id":"turn","request_id":"request","attempt":1,
                "span_id":format!("span-{ordinal}"),"parent_span_id":null,"phase":"model_request",
                "event":"started","at_ms":1000,"model":"test"
            }
        })
    };
    let mut file = std::fs::File::create(&parent).unwrap();
    writeln!(file, "{}", record(parent_id, 1)).unwrap();
    let cutoff = file.metadata().unwrap().len();
    writeln!(file, "{}", record(parent_id, 2)).unwrap();
    writeln!(
        std::fs::File::create(&child).unwrap(),
        "{}",
        record(child_id, 3)
    )
    .unwrap();
    let segments = vec![
        Segment {
            id: parent_id,
            path: parent,
            start_ordinal: Some(1),
            end_ordinal: Some(2),
            end_byte_offset: Some(cutoff),
        },
        Segment {
            id: child_id,
            path: child,
            start_ordinal: Some(3),
            end_ordinal: None,
            end_byte_offset: None,
        },
    ];
    let cursor = Cursor {
        version: 1,
        thread_id: child_id,
        turn_id: None,
        revision: 1,
        segment: 0,
        offset: 0,
    };
    let first = scan(segments.clone(), cursor, /*limit*/ 1, || false).unwrap();
    assert_eq!(first.data[0].record.span_id, "span-1");
    assert!(first.next_cursor.is_some());
    let cursor = serde_json::from_str(&first.cursor).unwrap();
    let second = scan(segments, cursor, /*limit*/ 10, || false).unwrap();
    assert_eq!(
        second
            .data
            .iter()
            .map(|entry| entry.record.span_id.as_str())
            .collect::<Vec<_>>(),
        vec!["span-3"]
    );
    assert_eq!(second.data[0].record.thread_id, child_id.to_string());
    assert_eq!(second.next_cursor, None);
}
