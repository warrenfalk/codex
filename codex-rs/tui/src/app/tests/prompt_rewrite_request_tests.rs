use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn prompt_rewrite_requests_reject_without_visible_events_or_pending_approvals() -> Result<()>
{
    let (mut app, mut app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let (app_server, requests, proxy) = session_lifecycle_requests::start_recording_app_server(
        &app.config,
        /*blocked_thread_list*/ None,
        /*failed_thread_name*/ None,
    )
    .await?;
    let thread_id = ThreadId::new();
    app.prompt_rewrite_thread_tombstones.insert(thread_id);
    while app_event_rx.try_recv().is_ok() {}

    for active in [true, false] {
        if active {
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            app.temporary_structured_requests.insert(thread_id, sender);
        } else {
            app.temporary_structured_requests.remove(&thread_id);
        }
        for request in [
            exec_approval_request(thread_id, "turn", "item", /*approval_id*/ None),
            request_user_input_request(thread_id, "turn", "item"),
        ] {
            app.handle_app_server_event(
                &app_server,
                codex_app_server_client::AppServerEvent::ServerRequest(Box::new(request.clone())),
            )
            .await;
            assert!(
                !app.pending_app_server_requests
                    .contains_server_request(&request)
            );
        }
        assert!(app_event_rx.try_recv().is_err());
        assert!(app.thread_event_channels.is_empty());
    }

    app_server.shutdown().await?;
    proxy.await??;
    assert_eq!(
        requests
            .lock()
            .expect("request recorder lock")
            .iter()
            .filter(|request| request.method == "server/request/error")
            .map(|request| (request.id.clone(), request.params.clone()))
            .collect::<Vec<_>>(),
        [1, 2, 1, 2]
            .map(|id| (
                AppServerRequestId::Integer(id),
                Some(serde_json::json!({
                    "code": -32000,
                    "message": "Prompt rewrite turns cannot call tools."
                }))
            ))
            .to_vec()
    );
    Ok(())
}
