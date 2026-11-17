use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn sync() -> ThreadMetadataSync {
    ThreadMetadataSync {
        thread_id: ThreadId::new(),
        cwd_seen: true,
        preview_seen: true,
        first_user_message_seen: true,
        title_seen: true,
        pending_update: None,
        pending_update_generation: 0,
        last_touch_persisted_at: None,
        defer_create_update_until_history_exists: true,
        defer_resume_update_until_append: true,
    }
}

fn timing_item(thread_id: ThreadId) -> RolloutItem {
    serde_json::from_value(json!({
        "type": "timing_record",
        "payload": {
            "version": 1, "thread_id": thread_id, "turn_id": "turn",
            "request_id": "request", "attempt": 1, "span_id": "span", "parent_span_id": null,
            "phase": "model_request", "event": "started", "at_ms": 1, "model": "model",
        },
    }))
    .unwrap()
}

#[test]
fn timing_only_appends_do_not_create_recency_touches() {
    let mut sync = sync();
    assert!(
        sync.observe_appended_items(&[timing_item(sync.thread_id)])
            .is_none()
    );
    assert!(sync.pending_update.is_none());
    assert!(sync.last_touch_persisted_at.is_none());
    assert_eq!(sync.pending_update_generation, 0);
    assert!(sync.defer_create_update_until_history_exists);
    assert!(sync.defer_resume_update_until_append);
}

#[test]
fn timing_only_appends_preserve_deferred_metadata_updates() {
    let mut sync = sync();
    sync.pending_update = Some(ThreadMetadataPatch {
        title: Some("deferred title".to_string()),
        ..Default::default()
    });
    sync.pending_update_generation = 7;
    assert!(
        sync.observe_appended_items(&[timing_item(sync.thread_id)])
            .is_none()
    );
    assert!(sync.take_pending_update_for_existing_history().is_none());
    let pending = sync.take_pending_update().unwrap();
    assert_eq!(
        (pending.generation, pending.patch.title.as_deref()),
        (7, Some("deferred title"))
    );
}
