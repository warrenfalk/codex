//! Public JSON-RPC coverage for controller isolation and human takeover.
use super::connection_handling_websocket::*;
use anyhow::Result;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::*;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::process::Child;
use tokio::sync::oneshot;

struct Fixture {
    _home: TempDir,
    _process: Child,
    address: std::net::SocketAddr,
    user: WsClient,
    dot: WsClient,
    thread: String,
}

async fn rpc<T: DeserializeOwned>(client: &mut WsClient, method: &str, params: Value) -> Result<T> {
    send_request(client, method, /*id*/ 100, Some(params)).await?;
    loop {
        match read_jsonrpc_message(client).await? {
            JSONRPCMessage::Response(response) if response.id == RequestId::Integer(100) => {
                return Ok(serde_json::from_value(response.result)?);
            }
            JSONRPCMessage::Error(error) if error.id == RequestId::Integer(100) => {
                anyhow::bail!("{method} failed: {:?}", error.error);
            }
            _ => {}
        }
    }
}

async fn initialized(address: std::net::SocketAddr, name: &str) -> Result<WsClient> {
    let mut client = connect_websocket(address).await?;
    send_initialize_request(&mut client, /*id*/ 1, name).await?;
    read_response_for_id(&mut client, /*id*/ 1).await?;
    Ok(client)
}

impl Fixture {
    async fn new(uri: &str) -> Result<Self> {
        let home = TempDir::new()?;
        create_config_toml(home.path(), uri, "on-request")?;
        let (process, address) = spawn_websocket_server(home.path()).await?;
        let mut user = initialized(address, "human").await?;
        let started: ThreadStartResponse = rpc(
            &mut user,
            "thread/start",
            json!({"approvalsReviewer":"user"}),
        )
        .await?;
        let thread = started.thread.id;
        let mut dot = initialized(address, "mcp-bridge").await?;
        let _: ControllerRegisterResponse = rpc(
            &mut dot,
            "controller/register",
            json!({"controllerId":"dot"}),
        )
        .await?;
        Ok(Self {
            _home: home,
            _process: process,
            address,
            user,
            dot,
            thread,
        })
    }

    async fn request(&mut self, id: &str) -> Result<ThreadControlState> {
        let response: ThreadControlRequestResponse = rpc(
            &mut self.dot,
            "thread/control/request",
            json!({
                "threadId": self.thread, "requestId": id, "reason":"Continue this task"
            }),
        )
        .await?;
        Ok(response.state)
    }

    async fn update(&mut self, action: Value) -> Result<ThreadControlState> {
        let response: ThreadControlUpdateResponse = rpc(
            &mut self.user,
            "thread/control/update",
            json!({
                "threadId":self.thread, "action":action
            }),
        )
        .await?;
        Ok(response.state)
    }
}

#[tokio::test]
async fn control_requests_are_idempotent_and_human_decisions_are_authoritative() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let mut f = Fixture::new(&server.uri()).await?;
    let pending = f.request("request-1").await?;
    assert_eq!(
        pending.request.as_ref().expect("control request").status,
        ThreadControlRequestStatus::Pending
    );
    assert!(pending.interactive_attached);
    assert!(
        pending
            .request
            .as_ref()
            .expect("control request")
            .auto_grant_at
            .is_some()
    );
    assert_eq!(f.request("request-1").await?, pending);
    let denied = f
        .update(json!({"type":"deny","requestId":"request-1"}))
        .await?;
    assert_eq!(
        denied.request.as_ref().expect("control request").status,
        ThreadControlRequestStatus::Denied
    );
    assert_eq!(f.request("request-1").await?, denied);
    f.request("request-2").await?;
    let granted = f
        .update(json!({"type":"approve","requestId":"request-2"}))
        .await?;
    assert_eq!(
        granted.grant.as_ref().expect("control grant").controller_id,
        "dot"
    );
    let old_grant = granted.grant.expect("control grant").id;
    let locked = f.update(json!({"type":"lock"})).await?;
    assert!(locked.locked);
    assert_eq!(locked.grant, None);
    let pending = f.request("request-3").await?;
    assert_eq!(
        pending.request.expect("control request").auto_grant_at,
        None
    );
    let granted = f
        .update(json!({"type":"approve","requestId":"request-3"}))
        .await?;
    assert!(granted.locked);
    assert_ne!(granted.grant.expect("control grant").id, old_grant);
    Ok(())
}

#[tokio::test]
async fn controller_cannot_change_permissions_approve_or_impersonate_human_input() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let mut f = Fixture::new(&server.uri()).await?;
    let granted = f
        .update(json!({"type":"grant","controllerId":"dot"}))
        .await?;
    let grant = granted.grant.expect("control grant").id;
    for (method, params) in [
        (
            "turn/start",
            json!({"threadId":f.thread,"input":[],"approvalPolicy":"never"}),
        ),
        (
            "thread/control/update",
            json!({"threadId":f.thread,"action":{"type":"unlock"}}),
        ),
        (
            "thread/resume",
            json!({"threadId":f.thread,"approvalPolicy":"never"}),
        ),
        (
            "thread/settings/update",
            json!({"threadId":f.thread,"approvalPolicy":"never"}),
        ),
        ("command/exec", json!({"command":["true"]})),
        ("controller/register", json!({"controllerId":"human"})),
        (
            "thread/control/input",
            json!({"threadId":f.thread,"grantId":grant,"input":[],"approvalPolicy":"never"}),
        ),
    ] {
        send_request(&mut f.dot, method, /*id*/ 101, Some(params)).await?;
        let error = read_error_for_id(&mut f.dot, /*id*/ 101).await?;
        assert!(error.error.code < 0, "{method} must be rejected");
    }
    let read: ThreadReadResponse =
        rpc(&mut f.dot, "thread/read", json!({"threadId":f.thread})).await?;
    assert_eq!(read.thread.id, f.thread);
    let mut other = initialized(f.address, "other-bridge").await?;
    let _: ControllerRegisterResponse = rpc(
        &mut other,
        "controller/register",
        json!({"controllerId":"claude"}),
    )
    .await?;
    for (method, params) in [
        (
            "thread/control/request",
            json!({"threadId":f.thread,"requestId":"other-request","reason":"Help"}),
        ),
        (
            "thread/control/input",
            json!({"threadId":f.thread,"grantId":grant,"input":[{"type":"text","text":"Hello"}]}),
        ),
    ] {
        send_request(&mut other, method, /*id*/ 102, Some(params)).await?;
        read_error_for_id(&mut other, /*id*/ 102).await?;
    }
    Ok(())
}

#[tokio::test]
async fn human_input_revokes_control_without_interrupting_the_running_turn() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (server, _completion) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_response_created("response")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![
                responses::ev_assistant_message("answer", "Done"),
                responses::ev_completed("response"),
            ]),
        },
    ]])
    .await;
    let mut f = Fixture::new(server.uri()).await?;
    let granted = f
        .update(json!({"type":"grant","controllerId":"dot"}))
        .await?;
    let grant = granted.grant.expect("control grant").id;
    let started: ThreadControlInputResponse = rpc(
        &mut f.dot,
        "thread/control/input",
        json!({
            "threadId":f.thread, "grantId":grant, "input":[{"type":"text","text":"Work"}]
        }),
    )
    .await?;
    let state: ThreadControlReadResponse = rpc(
        &mut f.dot,
        "thread/control/read",
        json!({"threadId":f.thread}),
    )
    .await?;
    assert!(state.state.turn_running);
    // Mid-turn steering accepts input but does not issue an interrupt.
    let steered: TurnSteerResponse = rpc(
        &mut f.user,
        "turn/steer",
        json!({
            "threadId":f.thread,"expectedTurnId":started.turn_id,
            "input":[{"type":"text","text":"Keep going"}]
        }),
    )
    .await?;
    assert_eq!(steered.turn_id, started.turn_id);
    let state: ThreadControlReadResponse = rpc(
        &mut f.dot,
        "thread/control/read",
        json!({"threadId":f.thread}),
    )
    .await?;
    assert_eq!(state.state.grant, None);
    assert!(state.state.turn_running);
    send_request(
        &mut f.dot,
        "thread/control/interrupt",
        /*id*/ 103,
        Some(json!({
            "threadId":f.thread,"grantId":grant,"turnId":started.turn_id
        })),
    )
    .await?;
    read_error_for_id(&mut f.dot, /*id*/ 103).await?;
    let pending = f.request("after-user").await?;
    assert_eq!(
        pending.request.as_ref().expect("control request").status,
        ThreadControlRequestStatus::Pending
    );
    // Submitting another user message denies the pending request too.
    let _: TurnSteerResponse = rpc(
        &mut f.user,
        "turn/steer",
        json!({
            "threadId":f.thread,"expectedTurnId":started.turn_id,
            "input":[{"type":"text","text":"Still mine"}]
        }),
    )
    .await?;
    let state: ThreadControlReadResponse = rpc(
        &mut f.dot,
        "thread/control/read",
        json!({"threadId":f.thread}),
    )
    .await?;
    assert_eq!(
        state.state.request.expect("control request").status,
        ThreadControlRequestStatus::Denied
    );
    let _ = release.send(());
    Ok(())
}

#[tokio::test]
async fn idle_unattached_thread_grants_immediately_and_identity_survives_reconnect() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let mut f = Fixture::new(&server.uri()).await?;
    f.request("before-detach").await?;
    f.update(json!({"type":"deny","requestId":"before-detach"}))
        .await?;
    let _: ThreadUnsubscribeResponse = rpc(
        &mut f.user,
        "thread/unsubscribe",
        json!({"threadId":f.thread}),
    )
    .await?;
    // Controller attachment does not count as an interactive interface.
    let _: ThreadResumeResponse = rpc(
        &mut f.dot,
        "thread/resume",
        json!({"threadId":f.thread,"excludeTurns":true}),
    )
    .await?;
    let granted = f.request("idle").await?;
    assert!(!granted.interactive_attached);
    let grant = granted.grant.expect("control grant");
    f.dot = initialized(f.address, "reconnected-bridge").await?;
    let _: ControllerRegisterResponse = rpc(
        &mut f.dot,
        "controller/register",
        json!({"controllerId":"dot"}),
    )
    .await?;
    let released: ThreadControlReleaseResponse = rpc(
        &mut f.dot,
        "thread/control/release",
        json!({
            "threadId":f.thread,"grantId":grant.id
        }),
    )
    .await?;
    assert_eq!(released.state.grant, None);
    Ok(())
}

#[tokio::test]
async fn busy_unattached_thread_waits_and_authorized_controller_can_interrupt_user_turn()
-> Result<()> {
    let (release, gate) = oneshot::channel();
    let (server, _completion) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_response_created("response")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![responses::ev_completed("response")]),
        },
    ]])
    .await;
    let mut f = Fixture::new(server.uri()).await?;
    let started: TurnStartResponse = rpc(
        &mut f.user,
        "turn/start",
        json!({
            "threadId":f.thread,"input":[{"type":"text","text":"Work"}]
        }),
    )
    .await?;
    let _: ThreadUnsubscribeResponse = rpc(
        &mut f.user,
        "thread/unsubscribe",
        json!({"threadId":f.thread}),
    )
    .await?;
    let pending = f.request("busy").await?;
    assert!(!pending.interactive_attached);
    assert!(pending.turn_running);
    assert_eq!(
        pending.active_turn_id.as_deref(),
        Some(started.turn.id.as_str())
    );
    assert_eq!(
        pending.request.expect("control request").status,
        ThreadControlRequestStatus::Pending
    );
    let granted = f
        .update(json!({"type":"approve","requestId":"busy"}))
        .await?;
    let grant = granted.grant.expect("control grant").id;
    let steered: ThreadControlInputResponse = rpc(
        &mut f.dot,
        "thread/control/input",
        json!({
            "threadId":f.thread,"grantId":grant,"expectedTurnId":started.turn.id,
            "input":[{"type":"text","text":"Keep working until I interrupt"}]
        }),
    )
    .await?;
    assert_eq!(steered.turn_id, started.turn.id);
    // Starting without an expected turn must fail rather than steering current work.
    send_request(
        &mut f.dot,
        "thread/control/input",
        /*id*/ 104,
        Some(json!({
            "threadId":f.thread,"grantId":grant,"input":[{"type":"text","text":"Another task"}]
        })),
    )
    .await?;
    read_error_for_id(&mut f.dot, /*id*/ 104).await?;
    let _: ThreadControlInterruptResponse = rpc(
        &mut f.dot,
        "thread/control/interrupt",
        json!({
            "threadId":f.thread,"grantId":grant,"turnId":started.turn.id
        }),
    )
    .await?;
    let _ = release.send(());
    Ok(())
}

#[tokio::test]
async fn locks_survive_server_restart_but_grants_and_requests_do_not() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let mut f = Fixture::new(&server.uri()).await?;
    f.update(json!({"type":"lock"})).await?;
    // Grant in advance is allowed without unlocking the thread.
    let granted = f
        .update(json!({"type":"grant","controllerId":"dot"}))
        .await?;
    assert!(granted.locked);
    assert!(granted.grant.is_some());
    f._process.kill().await?;
    f._process.wait().await?;
    let (process, address) = spawn_websocket_server(f._home.path()).await?;
    f._process = process;
    let mut reader = initialized(address, "after-restart").await?;
    let read: ThreadControlReadResponse = rpc(
        &mut reader,
        "thread/control/read",
        json!({"threadId":f.thread}),
    )
    .await?;
    assert!(read.state.locked);
    assert_eq!((read.state.grant, read.state.request), (None, None));
    Ok(())
}

#[tokio::test]
async fn controller_cannot_receive_replayed_approvals_or_answer_them() -> Result<()> {
    let server = app_test_support::create_mock_responses_server_sequence(vec![
        app_test_support::create_escalated_command_execution_sse_response(
            vec!["echo".to_owned(), "ok".to_owned()],
            /*workdir*/ None,
            Some(1000),
            "approval",
        )?,
        app_test_support::create_final_assistant_message_sse_response("Done")?,
    ])
    .await;
    let mut f = Fixture::new(&server.uri()).await?;
    let granted = f
        .update(json!({"type":"grant","controllerId":"dot"}))
        .await?;
    send_request(
        &mut f.user,
        "turn/start",
        /*id*/ 203,
        Some(json!({
            "threadId":f.thread,"input":[{"type":"text","text":"Run the command"}]
        })),
    )
    .await?;
    // Human input revoked the advance grant; grant control while the turn is waiting.
    assert!(granted.grant.is_some());
    let mut started = false;
    let mut approval = None;
    while !started || approval.is_none() {
        match read_jsonrpc_message(&mut f.user).await? {
            JSONRPCMessage::Request(request) => approval = Some(request),
            JSONRPCMessage::Response(response) if response.id == RequestId::Integer(203) => {
                started = true
            }
            JSONRPCMessage::Error(error) if error.id == RequestId::Integer(203) => {
                anyhow::bail!("turn start failed: {:?}", error.error)
            }
            _ => {}
        }
    }
    let approval = approval.expect("human approval request");
    assert_eq!(approval.method, "item/commandExecution/requestApproval");
    let granted = f
        .update(json!({"type":"grant","controllerId":"dot"}))
        .await?;
    assert!(granted.turn_running);
    // Resuming normally replays pending approvals. Controllers must be excluded.
    send_request(
        &mut f.dot,
        "thread/resume",
        /*id*/ 201,
        Some(json!({"threadId":f.thread,"excludeTurns":true})),
    )
    .await?;
    loop {
        let message = read_jsonrpc_message(&mut f.dot).await?;
        assert!(
            !matches!(message, JSONRPCMessage::Request(_)),
            "approval was replayed to a controller"
        );
        if matches!(message, JSONRPCMessage::Response(ref r) if r.id == RequestId::Integer(201)) {
            break;
        }
    }
    // Even knowing the request ID cannot confer approval authority.
    send_jsonrpc(
        &mut f.dot,
        JSONRPCMessage::Response(JSONRPCResponse {
            id: approval.id.clone(),
            result: json!({"decision":"accept"}),
        }),
    )
    .await?;
    send_request(
        &mut f.dot,
        "thread/read",
        /*id*/ 202,
        Some(json!({"threadId":f.thread})),
    )
    .await?;
    let read: ThreadReadResponse = loop {
        let message = read_jsonrpc_message(&mut f.dot).await?;
        assert!(
            !matches!(message, JSONRPCMessage::Request(_)),
            "approval was replayed to a controller after resume"
        );
        if let JSONRPCMessage::Response(response) = message
            && response.id == RequestId::Integer(202)
        {
            break serde_json::from_value(response.result)?;
        }
    };
    assert!(
        matches!(read.thread.status, ThreadStatus::Active { ref active_flags }
        if active_flags.contains(&ThreadActiveFlag::WaitingOnApproval))
    );
    send_jsonrpc(
        &mut f.user,
        JSONRPCMessage::Response(JSONRPCResponse {
            id: approval.id,
            result: json!({"decision":"decline"}),
        }),
    )
    .await?;
    read_notification_for_method(&mut f.user, "turn/completed").await?;
    Ok(())
}

#[tokio::test]
async fn one_grant_allows_multiple_completed_turns() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let mut f = Fixture::new(&server.uri()).await?;
    let grant = f
        .update(json!({"type":"grant","controllerId":"dot"}))
        .await?
        .grant
        .expect("advance grant");
    for text in ["First task", "Second task"] {
        let _: ThreadControlInputResponse = rpc(
            &mut f.dot,
            "thread/control/input",
            json!({
                "threadId":f.thread,"grantId":grant.id,"input":[{"type":"text","text":text}]
            }),
        )
        .await?;
        tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 15), async {
            loop {
                let read: ThreadControlReadResponse = rpc(
                    &mut f.dot,
                    "thread/control/read",
                    json!({"threadId":f.thread}),
                )
                .await?;
                assert_eq!(read.state.grant, Some(grant.clone()));
                if !read.state.turn_running {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(/*millis*/ 10)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
    }
    Ok(())
}
