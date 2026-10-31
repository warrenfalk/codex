use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_escalated_command_execution_sse_response;
use app_test_support::create_mock_responses_server_sequence;
use app_test_support::create_request_user_input_sse_response;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadReadStateOperation;
use codex_app_server_protocol::ThreadReadStateUpdateParams;
use codex_app_server_protocol::ThreadReadStateUpdateResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadStatusChangedNotification;
use codex_app_server_protocol::ThreadUnreadSetParams;
use codex_app_server_protocol::ThreadUnreadSetResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnSteerParams;
use codex_app_server_protocol::UserInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use core_test_support::PathExt;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(15);

async fn set_unread(
    client: &mut TestAppServer,
    thread_id: &str,
    unread: bool,
) -> Result<ThreadUnreadSetResponse> {
    client
        .request(|request_id| ClientRequest::ThreadUnreadSet {
            request_id,
            params: ThreadUnreadSetParams {
                thread_id: thread_id.to_string(),
                unread,
            },
        })
        .await
}

async fn read_thread(client: &mut TestAppServer, thread_id: &str) -> Result<ThreadReadResponse> {
    client
        .request(|request_id| ClientRequest::ThreadRead {
            request_id,
            params: ThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: false,
            },
        })
        .await
}

#[tokio::test]
async fn prompt_arms_unread_and_running_acknowledgements_are_discarded() -> Result<()> {
    for history_mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let (release, gate) = oneshot::channel();
        let (server, _completions) = start_streaming_sse_server(vec![
            vec![
                StreamingSseChunk {
                    gate: None,
                    body: responses::sse(vec![responses::ev_response_created("resp-1")]),
                },
                StreamingSseChunk {
                    gate: Some(gate),
                    body: responses::sse(vec![
                        responses::ev_assistant_message("answer", "Done"),
                        responses::ev_completed("resp-1"),
                    ]),
                },
            ],
            vec![StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![
                    responses::ev_response_created("resp-2"),
                    responses::ev_assistant_message("answer-2", "Done again"),
                    responses::ev_completed("resp-2"),
                ]),
            }],
        ])
        .await;
        let home = TempDir::new()?;
        MockResponsesConfig::new(server.uri()).write(home.path())?;
        let mut client = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
        let started = client
            .start_thread(ThreadStartParams {
                history_mode: Some(history_mode),
                ..Default::default()
            })
            .await?;
        let id = started.thread.id;
        assert_eq!(started.thread.unread, false);
        let _: TurnStartResponse = client
            .request(|request_id| ClientRequest::TurnStart {
                request_id,
                params: TurnStartParams {
                    thread_id: id.clone(),
                    input: vec![UserInput::Text {
                        text: "Work".into(),
                        text_elements: Vec::new(),
                    }],
                    ..Default::default()
                },
            })
            .await?;
        let mut saw_active = false;
        loop {
            let message = timeout(TIMEOUT, client.read_next_message()).await??;
            if let JSONRPCMessage::Notification(notification) = message {
                if notification.method == "thread/status/changed" {
                    let status: ThreadStatusChangedNotification =
                        serde_json::from_value(notification.params.unwrap())?;
                    saw_active |= matches!(status.status, ThreadStatus::Active { .. });
                } else if notification.method == "thread/unread/changed" {
                    assert!(
                        saw_active,
                        "watchers must see active before a newly armed unread flag"
                    );
                    break;
                }
            }
        }
        let active = read_thread(&mut client, &id).await?;
        assert!(active.thread.unread);
        let active_receipt = active
            .read_state
            .expect("shared receipt for accepted input");
        let modern: ThreadReadStateUpdateResponse = client
            .request(|request_id| ClientRequest::ThreadReadStateUpdate {
                request_id,
                params: ThreadReadStateUpdateParams {
                    thread_id: id.clone(),
                    operation: ThreadReadStateOperation::Read,
                    expected_revision: active_receipt.revision.clone(),
                },
            })
            .await?;
        assert_eq!(
            modern.read_state, active_receipt,
            "both APIs ignore active acknowledgements"
        );
        assert_eq!(
            set_unread(&mut client, &id, /*unread*/ false).await?,
            ThreadUnreadSetResponse { unread: true }
        );
        release.send(()).unwrap();
        let _: TurnCompletedNotification =
            timeout(TIMEOUT, client.read_notification("turn/completed")).await??;
        let stopped = read_thread(&mut client, &id).await?.thread;
        assert_eq!((stopped.unread, stopped.status), (true, ThreadStatus::Idle));
        let list_params: ThreadListParams =
            serde_json::from_value(serde_json::json!({"useStateDbOnly": true}))?;
        let listed: ThreadListResponse = client
            .request(|request_id| ClientRequest::ThreadList {
                request_id,
                params: list_params,
            })
            .await?;
        assert_eq!(
            listed
                .data
                .iter()
                .map(|thread| (&thread.id, thread.unread))
                .collect::<Vec<_>>(),
            vec![(&id, true)]
        );
        assert_eq!(
            set_unread(&mut client, &id, /*unread*/ false).await?,
            ThreadUnreadSetResponse { unread: false }
        );
        assert_eq!(read_thread(&mut client, &id).await?.thread.unread, false);
        let rejected = client
            .send_turn_steer_request(TurnSteerParams {
                thread_id: id.clone(),
                input: vec![UserInput::Text {
                    text: "Rejected steering".into(),
                    text_elements: Vec::new(),
                }],
                expected_turn_id: "missing-turn".into(),
                client_user_message_id: None,
                responsesapi_client_metadata: None,
                responsesapi_client_metadata_patch: None,
                additional_context: None,
            })
            .await?;
        let error: JSONRPCError = timeout(
            TIMEOUT,
            client.read_stream_until_error_message(RequestId::Integer(rejected)),
        )
        .await??;
        assert_eq!(error.error.code, -32600);
        assert_eq!(read_thread(&mut client, &id).await?.thread.unread, false);
        assert_eq!(
            set_unread(&mut client, &id, /*unread*/ true).await?,
            ThreadUnreadSetResponse { unread: true }
        );
        assert_eq!(
            set_unread(&mut client, &id, /*unread*/ false).await?,
            ThreadUnreadSetResponse { unread: false }
        );
        client
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: id.clone(),
                input: vec![UserInput::Text {
                    text: "Work again".into(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        assert_eq!(read_thread(&mut client, &id).await?.thread.unread, true);
        server.shutdown().await;
    }
    Ok(())
}

#[tokio::test]
async fn acknowledgement_waits_for_working_descendants_without_deferring() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (server, _completions) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_response_created("child")]),
        },
        StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![
                responses::ev_assistant_message("answer", "Done"),
                responses::ev_completed("child"),
            ]),
        },
    ]])
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(server.uri()).write(home.path())?;
    let mut client = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let root = client
        .start_thread(ThreadStartParams::default())
        .await?
        .thread
        .id;
    let child = client
        .start_thread(ThreadStartParams::default())
        .await?
        .thread
        .id;
    set_unread(&mut client, &root, /*unread*/ true).await?;
    let state = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(home.path().abs()),
        "mock_provider".into(),
    )
    .await?;
    state
        .upsert_thread_spawn_edge(
            codex_protocol::ThreadId::from_string(&root)?,
            codex_protocol::ThreadId::from_string(&child)?,
            codex_state::DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await?;
    let _: TurnStartResponse = client
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: child.clone(),
                input: vec![UserInput::Text {
                    text: "Work".into(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(
        set_unread(&mut client, &root, /*unread*/ false).await?,
        ThreadUnreadSetResponse { unread: true }
    );
    release.send(()).unwrap();
    let _: TurnCompletedNotification =
        timeout(TIMEOUT, client.read_notification("turn/completed")).await??;
    assert_eq!(read_thread(&mut client, &root).await?.thread.unread, true);
    assert_eq!(
        set_unread(&mut client, &root, /*unread*/ false).await?,
        ThreadUnreadSetResponse { unread: false }
    );
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn waiting_for_input_or_approval_rejects_acknowledgement_and_interruption_keeps_unread()
-> Result<()> {
    for waiting_on_approval in [false, true] {
        let request = if waiting_on_approval {
            create_escalated_command_execution_sse_response(
                vec!["echo".into(), "ok".into()],
                /*workdir*/ None,
                Some(1000),
                "approval",
            )?
        } else {
            create_request_user_input_sse_response("question")?
        };
        let server = create_mock_responses_server_sequence(vec![request]).await;
        let home = TempDir::new()?;
        MockResponsesConfig::new(&server.uri())
            .with_approval_policy("on-request")
            .with_root_config("approvals_reviewer = \"user\"")
            .write(home.path())?;
        let mut client = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
        let id = client
            .start_thread(ThreadStartParams::default())
            .await?
            .thread
            .id;
        let started: TurnStartResponse = client
            .request(|request_id| ClientRequest::TurnStart {
                request_id,
                params: TurnStartParams {
                    thread_id: id.clone(),
                    input: vec![UserInput::Text {
                        text: "Ask a question".into(),
                        text_elements: Vec::new(),
                    }],
                    collaboration_mode: (!waiting_on_approval).then(|| CollaborationMode {
                        mode: ModeKind::Plan,
                        settings: Settings {
                            model: "mock-model".into(),
                            reasoning_effort: None,
                            developer_instructions: None,
                        },
                    }),
                    ..Default::default()
                },
            })
            .await?;
        let request = timeout(TIMEOUT, client.read_stream_until_request_message()).await??;
        assert!(matches!(
            request,
            ServerRequest::ToolRequestUserInput { .. }
                | ServerRequest::CommandExecutionRequestApproval { .. }
        ));
        assert_eq!(
            set_unread(&mut client, &id, /*unread*/ false).await?,
            ThreadUnreadSetResponse { unread: true }
        );
        let _: TurnInterruptResponse = client
            .request(|request_id| ClientRequest::TurnInterrupt {
                request_id,
                params: TurnInterruptParams {
                    thread_id: id.clone(),
                    turn_id: started.turn.id,
                },
            })
            .await?;
        assert_eq!(read_thread(&mut client, &id).await?.thread.unread, true);
        assert_eq!(
            set_unread(&mut client, &id, /*unread*/ false).await?,
            ThreadUnreadSetResponse { unread: false }
        );
    }
    Ok(())
}
