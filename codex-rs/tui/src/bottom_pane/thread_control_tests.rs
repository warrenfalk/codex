use super::*;
use codex_app_server_protocol::ThreadControlGrant;
use codex_app_server_protocol::ThreadControlRequest;
use pretty_assertions::assert_eq;

fn state() -> ThreadControlState {
    ThreadControlState {
        thread_id: "thread".to_owned(),
        locked: false,
        grant: None,
        request: Some(ThreadControlRequest {
            id: "request".to_owned(),
            controller_id: "Dot".to_owned(),
            reason: "Finish the build and report the result.".to_owned(),
            status: ThreadControlRequestStatus::Pending,
            requested_at: 100,
            auto_grant_at: Some(160),
        }),
        interactive_attached: true,
        active_turn_id: Some("turn".to_owned()),
        turn_running: true,
    }
}

#[test]
fn control_notices_snapshot() {
    let pending = state();
    let mut locked = state();
    locked.locked = true;
    locked
        .request
        .as_mut()
        .expect("control request")
        .auto_grant_at = None;
    let mut granted = state();
    granted.request = None;
    granted.grant = Some(ThreadControlGrant {
        id: "grant".to_owned(),
        controller_id: "Dot".to_owned(),
    });
    let snapshots = [pending, locked, granted]
        .iter()
        .map(|state| {
            let notice = ControlNotice::new(state, /*now*/ 115);
            let area = Rect::new(
                /*x*/ 0,
                /*y*/ 0,
                /*width*/ 76,
                notice.desired_height(/*width*/ 76),
            );
            let mut buffer = Buffer::empty(area);
            notice.render(area, &mut buffer);
            buffer
                .content
                .chunks(/*chunk_size*/ 76)
                .map(|row| {
                    row.iter()
                        .map(ratatui::buffer::Cell::symbol)
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    insta::assert_snapshot!(snapshots);
}

#[test]
fn control_notice_preserves_draft_and_remains_visible_during_turn() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut pane = super::super::tests::test_pane(AppEventSender::new(tx));
    pane.set_composer_text("draft stays here".to_owned(), Vec::new(), Vec::new());
    pane.set_task_running(/*running*/ true);
    pane.set_thread_control_state(Some(state()));
    assert_eq!(pane.composer_text(), "draft stays here");
    let area = Rect::new(
        /*x*/ 0,
        /*y*/ 0,
        /*width*/ 76,
        pane.desired_height(/*width*/ 76),
    );
    let mut buffer = Buffer::empty(area);
    pane.render(area, &mut buffer);
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("Dot requests control"));
    assert!(rendered.contains("draft stays here"));
}
