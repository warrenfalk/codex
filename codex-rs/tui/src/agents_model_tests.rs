use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadArchivedNotification;
use codex_app_server_protocol::ThreadStartedNotification;
use codex_app_server_protocol::ThreadStatusChangedNotification;
use codex_app_server_protocol::ThreadUnarchivedNotification;
use codex_app_server_protocol::ThreadUnreadChangedNotification;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn unread_notifications_win_over_in_flight_reads_in_both_directions() {
    let id = ThreadId::new();
    let mut thread: Thread = serde_json::from_value(json!({
        "id": id.to_string(), "sessionId": id.to_string(), "preview": "Task",
        "ephemeral": false, "modelProvider": "openai", "createdAt": 1, "updatedAt": 2,
        "status": {"type": "idle"}, "cwd": test_path_buf("/project").abs(),
        "cliVersion": "0.0.0", "source": "cli", "turns": []
    }))
    .unwrap();
    let mut model = AgentsModel::default();
    for unread in [true, false] {
        model.threads.insert(id, Some(thread.clone()));
        let request = model.begin_refresh().unwrap();
        model.observe(&ServerNotification::ThreadUnreadChanged(
            ThreadUnreadChangedNotification {
                thread_id: id.to_string(),
                unread,
            },
        ));
        model
            .finish_refresh(
                request.id,
                Ok(AgentsRefresh {
                    threads: HashMap::from([(id, Some(thread.clone()))]),
                    recent_seed_complete: true,
                    ..Default::default()
                }),
            )
            .unwrap();
        thread.unread = unread;
        assert_eq!(model.threads, HashMap::from([(id, Some(thread.clone()))]));
    }
}

#[test]
fn refreshing_a_descendant_cannot_restore_an_archived_root_until_unarchived() {
    let root_id = ThreadId::new();
    let child_id = ThreadId::new();
    let root: Thread = serde_json::from_value(json!({
        "id": root_id.to_string(), "sessionId": root_id.to_string(),
        "preview": "Task", "ephemeral": false, "modelProvider": "openai",
        "createdAt": 1, "updatedAt": 2, "status": {"type": "idle"},
        "cwd": test_path_buf("/project").abs(), "cliVersion": "0.0.0",
        "source": "cli", "turns": []
    }))
    .unwrap();
    let mut child = root.clone();
    child.id = child_id.to_string();
    child.parent_thread_id = Some(root_id.to_string());
    let mut model = AgentsModel::default();
    for thread in [root.clone(), child.clone()] {
        model.observe(&ServerNotification::ThreadStarted(
            ThreadStartedNotification { thread },
        ));
    }
    let request = model.begin_refresh().unwrap();
    model
        .finish_refresh(
            request.id,
            Ok(AgentsRefresh {
                recent_seed_complete: true,
                ..Default::default()
            }),
        )
        .unwrap();
    model.observe(&ServerNotification::ThreadArchived(
        ThreadArchivedNotification {
            thread_id: root_id.to_string(),
        },
    ));
    model.observe(&ServerNotification::ThreadStatusChanged(
        ThreadStatusChangedNotification {
            thread_id: child_id.to_string(),
            status: ThreadStatus::Idle,
        },
    ));
    let request = model.begin_refresh().unwrap();
    let records = HashMap::from([(root_id, Some(root)), (child_id, Some(child.clone()))]);
    // An ancestor read or a stale discovery page may still contain archived metadata.
    model
        .finish_refresh(
            request.id,
            Ok(AgentsRefresh {
                threads: records.clone(),
                recent_seed_complete: true,
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(model.threads, HashMap::from([(child_id, Some(child))]));

    model.observe(&ServerNotification::ThreadUnarchived(
        ThreadUnarchivedNotification {
            thread_id: root_id.to_string(),
        },
    ));
    let request = model.begin_refresh().unwrap();
    model
        .finish_refresh(
            request.id,
            Ok(AgentsRefresh {
                threads: records.clone(),
                recent_seed_complete: true,
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(model.threads, records);
}
