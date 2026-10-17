use super::*;
use pretty_assertions::assert_eq;
use std::os::unix::fs::PermissionsExt;

fn registry() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(/*mode*/ 0o700))
        .tempdir_in("/tmp")
        .unwrap()
}

fn endpoint() -> RemoteAppServerEndpoint {
    RemoteAppServerEndpoint::WebSocket {
        websocket_url: "ws://localhost:5000/".into(),
        auth_token: Some("not-sent-to-tuis".into()),
    }
}

fn mock_tui(directory: &Path, name: &str, replies: Vec<Reply>) -> JoinHandle<Vec<Action>> {
    let listener = UnixListener::bind(directory.join(name)).unwrap();
    tokio::spawn(async move {
        let mut actions = Vec::new();
        for reply in replies {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let query: Request = read_message(&mut stream).await.unwrap();
            assert_eq!(
                (query.endpoint, query.session_id),
                (Endpoint::from(&endpoint()), "session".into())
            );
            actions.push(query.action);
            let bytes = format!("{}\n", serde_json::to_string(&reply).unwrap());
            stream.get_mut().write_all(bytes.as_bytes()).await.unwrap();
        }
        actions
    })
}

#[tokio::test]
async fn selects_first_focusable_tui_in_pid_order_and_ignores_stale_sockets() {
    let dir = registry();
    drop(UnixListener::bind(dir.path().join("0000000000-stale.sock")).unwrap());
    let other = mock_tui(dir.path(), "0000000001-other.sock", vec![Reply::NoMatch]);
    let unsupported = mock_tui(
        dir.path(),
        "0000000002-unsupported.sock",
        vec![Reply::Unavailable("no Kitty".into())],
    );
    let selected = mock_tui(
        dir.path(),
        "0000000003-selected.sock",
        vec![Reply::Ready, Reply::Focused],
    );
    let unused = mock_tui(dir.path(), "0000000004-unused.sock", vec![Reply::Ready]);
    focus(dir.path(), endpoint(), "session").await.unwrap();
    assert_eq!(
        (
            other.await.unwrap(),
            unsupported.await.unwrap(),
            selected.await.unwrap()
        ),
        (
            vec![Action::Probe],
            vec![Action::Probe],
            vec![Action::Probe, Action::Focus]
        )
    );
    assert!(!unused.is_finished());
    unused.abort();
}

#[tokio::test]
async fn rechecks_session_after_probe_and_does_not_retry_failed_focus() {
    let dir = registry();
    let switched = mock_tui(
        dir.path(),
        "0000000001-switched.sock",
        vec![Reply::Ready, Reply::NoMatch],
    );
    let failed = mock_tui(
        dir.path(),
        "0000000002-failed.sock",
        vec![
            Reply::Ready,
            Reply::Unavailable("remote control disabled".into()),
        ],
    );
    let unused = mock_tui(dir.path(), "0000000003-unused.sock", vec![Reply::Ready]);
    let error = focus(dir.path(), endpoint(), "session").await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Could not focus TUI: remote control disabled"
    );
    assert_eq!(
        (switched.await.unwrap(), failed.await.unwrap()),
        (
            vec![Action::Probe, Action::Focus],
            vec![Action::Probe, Action::Focus]
        )
    );
    assert!(!unused.is_finished());
    unused.abort();
}

#[tokio::test]
async fn reports_no_match_unavailable_and_insecure_registry() {
    let dir = registry();
    let absent = dir.path().join("absent");
    assert!(
        focus(&absent, endpoint(), "session")
            .await
            .unwrap_err()
            .to_string()
            .contains("No running local TUI")
    );
    assert!(!absent.exists());
    let unavailable = mock_tui(
        dir.path(),
        "0000000001-unavailable.sock",
        vec![Reply::Unavailable("no Kitty".into())],
    );
    assert_eq!(
        focus(dir.path(), endpoint(), "session")
            .await
            .unwrap_err()
            .to_string(),
        "TUI focus unavailable: no Kitty"
    );
    unavailable.await.unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(/*mode*/ 0o755)).unwrap();
    assert!(
        focus(dir.path(), endpoint(), "session")
            .await
            .unwrap_err()
            .to_string()
            .contains("private directory")
    );
}

#[tokio::test]
async fn service_cancels_abandoned_requests_and_removes_registration_on_drop() {
    let dir = registry();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let service = FocusService::start(dir.path(), AppEventSender::new(tx)).unwrap();
    let socket = service.socket.clone();
    let query = Request {
        endpoint: Endpoint::from(&endpoint()),
        session_id: "session".into(),
        action: Action::Focus,
    };
    let (reply, ()) = tokio::join!(request(&socket, &query), async {
        let Some(AppEvent::AgentsFocusRequested(pending)) = rx.recv().await else {
            panic!("expected focus request")
        };
        pending.response.send(Reply::Focused).unwrap();
    });
    assert_eq!(reply.unwrap(), Reply::Focused);
    let mut stream = UnixStream::connect(&socket).await.unwrap();
    stream
        .write_all(format!("{}\n", serde_json::to_string(&query).unwrap()).as_bytes())
        .await
        .unwrap();
    let Some(AppEvent::AgentsFocusRequested(mut pending)) = rx.recv().await else {
        panic!("expected focus request")
    };
    drop(stream);
    tokio::time::timeout(REQUEST_TIMEOUT, pending.response.closed())
        .await
        .unwrap();
    drop(service);
    assert!(!socket.exists());
}

#[tokio::test(start_paused = true)]
async fn unresponsive_tui_has_a_bounded_failure() {
    let dir = registry();
    let listener = UnixListener::bind(dir.path().join("hung.sock")).unwrap();
    assert!(
        focus(dir.path(), endpoint(), "session")
            .await
            .unwrap_err()
            .to_string()
            .contains("did not respond")
    );
    drop(listener);
}
