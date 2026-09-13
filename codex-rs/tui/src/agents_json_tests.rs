use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::ThreadStatus;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

fn thread(id: &str, status: Value) -> Thread {
    serde_json::from_value(json!({
        "id": id, "sessionId": id, "preview": " Original prompt\nmore", "name": null,
        "ephemeral": false, "modelProvider": "openai", "createdAt": 1, "updatedAt": 2,
        "status": status, "cwd": test_path_buf("/project").abs(), "cliVersion": "0.0.0",
        "source": "cli", "turns": []
    }))
    .unwrap()
}

struct Mock {
    options: AgentsJsonOptions,
    threads: Arc<Mutex<BTreeMap<String, Thread>>>,
    commands: mpsc::UnboundedSender<Option<Value>>,
    archived: Arc<Mutex<BTreeSet<String>>>,
    read_errors: Arc<Mutex<BTreeMap<(String, String), Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Mock {
    async fn start(threads: Vec<Thread>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let options = AgentsJsonOptions {
            endpoint: RemoteAppServerEndpoint::WebSocket {
                websocket_url: format!("ws://{}", listener.local_addr().unwrap()),
                auth_token: None,
            },
            watch: false,
            worktree_grouping: false,
            implicit_local_daemon: false,
        };
        let threads = Arc::new(Mutex::new(
            threads
                .into_iter()
                .map(|thread| (thread.id.clone(), thread))
                .collect::<BTreeMap<_, _>>(),
        ));
        let (commands, mut command_rx) = mpsc::unbounded_channel::<Option<Value>>();
        let data = Arc::clone(&threads);
        let archived = Arc::new(Mutex::new(BTreeSet::<String>::new()));
        let server_archived = Arc::clone(&archived);
        let read_errors = Arc::new(Mutex::new(BTreeMap::<(String, String), Value>::new()));
        let server_read_errors = Arc::clone(&read_errors);
        let task = tokio::spawn(async move {
            let mut inject_race = true;
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let mut websocket = tokio_tungstenite::accept_async(socket).await.unwrap();
                let mut subscribed = false;
                loop {
                    let request = tokio::select! {
                        command = command_rx.recv() => {
                            match command {
                                Some(Some(notification)) => websocket.send(Message::Text(notification.to_string().into())).await.unwrap(),
                                Some(None) | None => { let _ = websocket.close(None).await; break; }
                            }
                            continue;
                        }
                        message = websocket.next() => match message {
                            Some(Ok(Message::Text(text))) => serde_json::from_str::<Value>(&text).unwrap(),
                            _ => break,
                        }
                    };
                    if request["method"] == "initialized" {
                        continue;
                    }
                    let params = &request["params"];
                    let method = request["method"].as_str().unwrap();
                    if method.starts_with("thread/") {
                        assert!(subscribed);
                    }
                    let error = server_read_errors
                        .lock()
                        .unwrap()
                        .get(&(
                            method.to_string(),
                            params["threadId"].as_str().unwrap_or_default().to_string(),
                        ))
                        .cloned();
                    if let Some(error) = error {
                        websocket
                            .send(Message::Text(
                                json!({"id": request["id"], "error": error})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                        continue;
                    }
                    let result = match method {
                        "initialize" => json!({"userAgent": "mock"}),
                        "event/firehose" => {
                            subscribed = true;
                            json!({})
                        }
                        "thread/list" | "thread/loaded/list" => {
                            let mut rows: Vec<Value> = data
                                .lock()
                                .unwrap()
                                .values()
                                .filter(|thread| {
                                    let archived =
                                        server_archived.lock().unwrap().contains(&thread.id);
                                    if method == "thread/loaded/list" {
                                        !archived
                                            && !matches!(thread.status, ThreadStatus::NotLoaded)
                                    } else {
                                        archived == (params["archived"] == true)
                                            && params["sourceKinds"] == json!([])
                                            && (archived || thread.id != "root")
                                    }
                                })
                                .map(|thread| {
                                    if method == "thread/loaded/list" {
                                        json!(thread.id)
                                    } else {
                                        json!(thread)
                                    }
                                })
                                .collect();
                            let start = params["cursor"]
                                .as_str()
                                .map(|cursor| cursor.parse::<usize>().unwrap())
                                .unwrap_or(0);
                            let next = (start + 1 < rows.len()).then(|| (start + 1).to_string());
                            rows = rows.into_iter().skip(start).take(1).collect();
                            json!({"data": rows, "nextCursor": next})
                        }
                        "thread/read" => {
                            let id = params["threadId"].as_str().unwrap();
                            let stale = data.lock().unwrap()[id].clone();
                            if id == "root" && inject_race {
                                inject_race = false;
                                data.lock().unwrap().get_mut(id).unwrap().name =
                                    Some("Renamed during initialization".into());
                                websocket.send(Message::Text(json!({"method": "thread/name/updated", "params": {"threadId": id, "threadName": "Renamed during initialization"}}).to_string().into())).await.unwrap();
                            }
                            json!({"thread": stale})
                        }
                        "thread/turns/list" => {
                            json!({"data": [{"id": "turn", "status": "completed", "items": [{"id": "user", "type": "userMessage", "content": [{"type": "text", "text": "Latest prompt\nSecond line"}]}]}], "nextCursor": null, "backwardsCursor": null})
                        }
                        _ => panic!("unexpected request from passive observer: {method}"),
                    };
                    if websocket
                        .send(Message::Text(
                            json!({"id": request["id"], "result": result})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
        Self {
            options,
            threads,
            commands,
            archived,
            read_errors,
            task,
        }
    }
}

#[tokio::test]
async fn one_shot_pages_and_reconciles_events_and_unloaded_ancestors() {
    let mut child = thread("child", json!({"type": "active", "activeFlags": []}));
    child.parent_thread_id = Some("root".into());
    let mut blocked = thread(
        "blocked",
        json!({"type": "active", "activeFlags": ["waitingOnApproval", "waitingOnUserInput"]}),
    );
    blocked.parent_thread_id = Some("child".into());
    let mut ephemeral = thread("ephemeral", json!({"type": "idle"}));
    ephemeral.ephemeral = true;
    let mock = Mock::start(vec![
        child,
        blocked,
        ephemeral,
        thread("root", json!({"type": "notLoaded"})),
        thread("finished", json!({"type": "notLoaded"})),
    ])
    .await;
    let mut output = Vec::new();
    run(&mock.options, &mut output).await.unwrap();
    assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
    let mut snapshot: Value = serde_json::from_slice(&output).unwrap();
    // Normalize only platform-dependent test paths in the visual contract.
    for session in snapshot["sessions"].as_array_mut().unwrap() {
        session["cwd"] = json!("/project");
        session["project"] = json!({"key": ["/project", ""], "heading": "/project"});
    }
    insta::assert_snapshot!(serde_json::to_string_pretty(&snapshot).unwrap());
}

struct Lines(mpsc::UnboundedSender<Value>, Vec<u8>);
impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.1.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        assert_eq!(self.1.last(), Some(&b'\n'));
        self.0
            .send(serde_json::from_slice(&self.1).unwrap())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        self.1.clear();
        Ok(())
    }
}

fn not_loaded_error(id: &str) -> Value {
    json!({"code": -32600, "message": format!("thread not loaded: {id}")})
}

#[tokio::test]
async fn watch_retains_unloaded_tasks_deduplicates_and_resynchronizes() {
    let mut mock = Mock::start(vec![thread("root", json!({"type": "idle"}))]).await;
    mock.options.watch = true;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut output = Lines(tx, Vec::new());
    let exercise = async {
        let initial = rx.recv().await.unwrap();
        assert_eq!(
            initial["counts"],
            json!({"total": 1, "working": 0, "needsAttention": 0})
        );
        mock.read_errors.lock().unwrap().insert(
            ("thread/read".into(), "root".into()),
            not_loaded_error("root"),
        );
        mock.commands.send(Some(json!({"method": "thread/status/changed", "params": {"threadId": "root", "status": {"type": "idle"}}}))).unwrap();
        mock.threads.lock().unwrap().get_mut("root").unwrap().status = ThreadStatus::NotLoaded;
        mock.commands
            .send(Some(
                json!({"method": "thread/closed", "params": {"threadId": "root"}}),
            ))
            .unwrap();
        let finished = rx.recv().await.unwrap();
        assert_eq!(
            (
                finished["sessions"][0]["status"].clone(),
                finished["sessions"][0]["loaded"].clone()
            ),
            (json!("finished"), json!(false))
        );
        mock.commands.send(None).unwrap();
        assert_eq!(
            rx.recv().await.unwrap(),
            json!({"version": 1, "connection": "disconnected", "counts": null, "sessions": null})
        );
        assert_eq!(rx.recv().await.unwrap(), finished);
        mock.read_errors.lock().unwrap().clear();
        mock.threads.lock().unwrap().get_mut("root").unwrap().status = ThreadStatus::SystemError;
        mock.commands.send(Some(json!({"method": "thread/status/changed", "params": {"threadId": "root", "status": {"type": "systemError"}}}))).unwrap();
        let reconnected = rx.recv().await.unwrap();
        assert_eq!(
            (
                reconnected["connection"].clone(),
                reconnected["counts"].clone(),
                reconnected["sessions"][0]["attention"].clone()
            ),
            (
                json!("connected"),
                json!({"total": 1, "working": 0, "needsAttention": 1}),
                json!(["error"])
            )
        );
        mock.commands.send(None).unwrap();
        assert_eq!(
            rx.recv().await.unwrap()["connection"],
            json!("disconnected")
        );
        mock.archived.lock().unwrap().insert("root".into());
        let empty = json!({"version": 1, "connection": "connected", "counts": {"total": 0, "working": 0, "needsAttention": 0}, "sessions": []});
        assert_eq!(rx.recv().await.unwrap(), empty);
        let new_thread = thread("new", json!({"type": "idle"}));
        mock.threads
            .lock()
            .unwrap()
            .insert("new".into(), new_thread.clone());
        mock.commands
            .send(Some(
                json!({"method": "thread/started", "params": {"thread": new_thread}}),
            ))
            .unwrap();
        assert_eq!(rx.recv().await.unwrap()["sessions"][0]["id"], json!("new"));
        let mut child = thread("descendant", json!({"type": "active", "activeFlags": []}));
        child.parent_thread_id = Some("new".into());
        mock.threads
            .lock()
            .unwrap()
            .insert(child.id.clone(), child.clone());
        mock.commands
            .send(Some(
                json!({"method": "thread/started", "params": {"thread": child}}),
            ))
            .unwrap();
        let family = rx.recv().await.unwrap();
        assert_eq!(family["counts"]["working"], json!(1));
        mock.commands
            .send(Some(
                json!({"method": "thread/archived", "params": {"threadId": "new"}}),
            ))
            .unwrap();
        assert_eq!(rx.recv().await.unwrap(), empty);
        mock.commands
            .send(Some(
                json!({"method": "thread/unarchived", "params": {"threadId": "new"}}),
            ))
            .unwrap();
        assert_eq!(rx.recv().await.unwrap(), family);
        mock.threads.lock().unwrap().remove("new");
        mock.commands
            .send(Some(
                json!({"method": "thread/deleted", "params": {"threadId": "new"}}),
            ))
            .unwrap();
        assert_eq!(rx.recv().await.unwrap(), empty);
    };
    tokio::select! {
        result = run(&mock.options, &mut output) => panic!("watch ended early: {result:?}"),
        () = exercise => {},
        () = tokio::time::sleep(Duration::from_secs(10)) => panic!("watch stalled"),
    }
}

#[tokio::test]
async fn watch_retains_started_threads_when_metadata_or_history_reads_race_unloading() {
    for method in ["thread/read", "thread/turns/list"] {
        let mut mock = Mock::start(Vec::new()).await;
        mock.options.watch = true;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut output = Lines(tx, Vec::new());
        let exercise = async {
            assert_eq!(
                rx.recv().await.unwrap(),
                json!({
                    "version": 1, "connection": "connected",
                    "counts": {"total": 0, "working": 0, "needsAttention": 0}, "sessions": []
                })
            );
            let task = thread("task", json!({"type": "active", "activeFlags": []}));
            mock.threads
                .lock()
                .unwrap()
                .insert(task.id.clone(), task.clone());
            mock.read_errors
                .lock()
                .unwrap()
                .insert((method.into(), task.id.clone()), not_loaded_error(&task.id));
            mock.commands
                .send(Some(json!({
                    "method": "thread/started", "params": {"thread": task}
                })))
                .unwrap();
            let mut expected = json!({
                "version": 1, "connection": "connected",
                "counts": {"total": 1, "working": 0, "needsAttention": 0},
                "sessions": [{
                    "id": "task", "title": "Original prompt", "cwd": task.cwd,
                    "name": null, "preview": task.preview, "createdAt": 1, "updatedAt": 2,
                    "recencyAt": null, "gitInfo": null, "status": "finished",
                    "rootStatus": {"type": "notLoaded"},
                    "project": {"key": [task.cwd, ""], "heading": task.cwd},
                    "loaded": false, "working": false, "attention": []
                }]
            });
            assert_eq!(rx.recv().await.unwrap(), expected);
            mock.read_errors.lock().unwrap().clear();
            mock.threads.lock().unwrap().get_mut("task").unwrap().status = ThreadStatus::Idle;
            mock.commands
                .send(Some(json!({
                    "method": "thread/status/changed", "params": {
                        "threadId": "task", "status": {"type": "idle"}
                    }
                })))
                .unwrap();
            expected["sessions"][0]["title"] = json!("Latest prompt");
            expected["sessions"][0]["preview"] = json!("Latest prompt\nSecond line");
            expected["sessions"][0]["status"] = json!("ready");
            expected["sessions"][0]["rootStatus"] = json!({"type": "idle"});
            expected["sessions"][0]["loaded"] = json!(true);
            assert_eq!(rx.recv().await.unwrap(), expected);
            mock.read_errors
                .lock()
                .unwrap()
                .insert((method.into(), task.id.clone()), not_loaded_error(&task.id));
            mock.commands
                .send(Some(json!({
                    "method": "thread/closed", "params": {"threadId": task.id}
                })))
                .unwrap();
            expected["sessions"][0]["status"] = json!("finished");
            expected["sessions"][0]["rootStatus"] = json!({"type": "notLoaded"});
            expected["sessions"][0]["loaded"] = json!(false);
            assert_eq!(rx.recv().await.unwrap(), expected);
        };
        tokio::select! {
            result = run(&mock.options, &mut output) => panic!("watch ended early: {result:?}"),
            () = exercise => {},
            () = tokio::time::sleep(Duration::from_secs(10)) => panic!("watch stalled"),
        }
    }
}

#[tokio::test]
async fn one_shot_retains_recent_metadata_and_skips_unknown_threads_that_unload_during_discovery() {
    let mock = Mock::start(vec![
        thread("recent", json!({"type": "active", "activeFlags": []})),
        // The mock excludes this ID from thread/list, so only its loaded ID is known.
        thread("root", json!({"type": "active", "activeFlags": []})),
    ])
    .await;
    for id in ["recent", "root"] {
        mock.read_errors
            .lock()
            .unwrap()
            .insert(("thread/read".into(), id.into()), not_loaded_error(id));
    }
    let mut output = Vec::new();
    run(&mock.options, &mut output).await.unwrap();
    let mut snapshot: Value = serde_json::from_slice(&output).unwrap();
    for session in snapshot["sessions"].as_array_mut().unwrap() {
        session["cwd"] = json!("/project");
        session["project"] = json!({"key": ["/project", ""], "heading": "/project"});
    }
    insta::assert_snapshot!(serde_json::to_string_pretty(&snapshot).unwrap());
}

#[tokio::test]
async fn authentication_failure_is_fatal_in_watch_mode_without_output() {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    for status in [401, 403] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let options = AgentsJsonOptions {
            endpoint: RemoteAppServerEndpoint::WebSocket {
                websocket_url: format!("ws://{}", listener.local_addr().unwrap()),
                auth_token: Some("test".into()),
            },
            watch: true,
            worktree_grouping: false,
            implicit_local_daemon: false,
        };
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut handshake = Vec::new();
            while !handshake.ends_with(b"\r\n\r\n") {
                handshake.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(
                    format!("HTTP/1.1 {status} Denied\r\nContent-Length: 0\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
        });
        let mut output = Vec::new();
        let error = tokio::time::timeout(Duration::from_secs(2), run(&options, &mut output))
            .await
            .unwrap()
            .unwrap_err();
        assert!(!retryable(&error));
        assert_eq!(output, Vec::<u8>::new());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn one_shot_connection_failure_has_no_snapshot() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let options = AgentsJsonOptions {
        endpoint: RemoteAppServerEndpoint::WebSocket {
            websocket_url: format!("ws://{}", listener.local_addr().unwrap()),
            auth_token: None,
        },
        watch: false,
        worktree_grouping: false,
        implicit_local_daemon: false,
    };
    drop(listener);
    let mut output = Vec::new();
    assert!(run(&options, &mut output).await.is_err());
    assert_eq!(output, Vec::<u8>::new());
}

#[tokio::test]
async fn interrupted_handshake_is_retryable() {
    use tokio::io::AsyncReadExt;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let options = AgentsJsonOptions {
        endpoint: RemoteAppServerEndpoint::WebSocket {
            websocket_url: format!("ws://{}", listener.local_addr().unwrap()),
            auth_token: None,
        },
        watch: false,
        worktree_grouping: false,
        implicit_local_daemon: false,
    };
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut handshake = Vec::new();
        while !handshake.ends_with(b"\r\n\r\n") {
            handshake.push(socket.read_u8().await.unwrap());
        }
    });
    let mut output = Vec::new();
    let error = run(&options, &mut output).await.unwrap_err();
    assert!(retryable(&error));
    assert_eq!(output, Vec::<u8>::new());
    server.await.unwrap();
}
