use super::*;
use codex_app_server_protocol::ConfigWarningNotification;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequestObservedNotification;
use codex_app_server_protocol::ThreadRealtimeStartedNotification;
use codex_protocol::protocol::RealtimeConversationVersion;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::time::Duration;
use tokio::time::timeout;

fn absolute_path(path: &str) -> AbsolutePathBuf {
    AbsolutePathBuf::from_absolute_path(path).expect("absolute path")
}

fn thread_realtime_started_notification() -> ServerNotification {
    ServerNotification::ThreadRealtimeStarted(ThreadRealtimeStartedNotification {
        thread_id: "thread-1".to_string(),
        realtime_session_id: None,
        version: RealtimeConversationVersion::V1,
    })
}

fn config_warning(summary: &str) -> ServerNotification {
    ServerNotification::ConfigWarning(ConfigWarningNotification {
        summary: summary.to_string(),
        details: None,
        path: None,
        range: None,
    })
}

fn initialized_connection(writer: mpsc::Sender<QueuedOutgoingMessage>) -> OutboundConnectionState {
    OutboundConnectionState::new(
        writer,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicBool::new(true)),
        Arc::new(RwLock::new(HashSet::new())),
        /*disconnect_sender*/ None,
    )
}

fn initialized_firehose_connection(
    writer: mpsc::Sender<QueuedOutgoingMessage>,
    opted_out_notification_methods: HashSet<String>,
) -> OutboundConnectionState {
    let connection_state = OutboundConnectionState::new(
        writer,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicBool::new(true)),
        Arc::new(RwLock::new(opted_out_notification_methods)),
        /*disconnect_sender*/ None,
    );
    connection_state.session.subscribe_firehose();
    connection_state
}

#[tokio::test]
async fn to_connection_notification_respects_opt_out_filters() {
    let connection_id = ConnectionId(7);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);
    let initialized = Arc::new(AtomicBool::new(true));
    let opted_out_notification_methods =
        Arc::new(RwLock::new(HashSet::from(["configWarning".to_string()])));

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            initialized,
            Arc::new(AtomicBool::new(true)),
            opted_out_notification_methods,
            /*disconnect_sender*/ None,
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
                ConfigWarningNotification {
                    summary: "task_started".to_string(),
                    details: None,
                    path: None,
                    range: None,
                },
            )),
            write_complete_tx: None,
        },
    )
    .await;

    assert!(
        writer_rx.try_recv().is_err(),
        "opted-out notification should be dropped"
    );
}

#[tokio::test]
async fn to_connection_notifications_are_dropped_for_opted_out_clients() {
    let connection_id = ConnectionId(10);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(RwLock::new(HashSet::from(["configWarning".to_string()]))),
            /*disconnect_sender*/ None,
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
                ConfigWarningNotification {
                    summary: "task_started".to_string(),
                    details: None,
                    path: None,
                    range: None,
                },
            )),
            write_complete_tx: None,
        },
    )
    .await;

    assert!(
        writer_rx.try_recv().is_err(),
        "opted-out notifications should not reach clients"
    );
}

#[tokio::test]
async fn to_connection_notifications_are_preserved_for_non_opted_out_clients() {
    let connection_id = ConnectionId(11);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(RwLock::new(HashSet::new())),
            /*disconnect_sender*/ None,
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
                ConfigWarningNotification {
                    summary: "task_started".to_string(),
                    details: None,
                    path: None,
                    range: None,
                },
            )),
            write_complete_tx: None,
        },
    )
    .await;

    let message = writer_rx
        .recv()
        .await
        .expect("notification should reach non-opted-out clients");
    assert!(matches!(
        message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "task_started"
    ));
}

#[tokio::test]
async fn broadcast_notifications_reach_firehose_connections_despite_opt_out() {
    let normal_connection_id = ConnectionId(21);
    let firehose_connection_id = ConnectionId(22);
    let (normal_writer_tx, mut normal_writer_rx) = mpsc::channel(2);
    let (firehose_writer_tx, mut firehose_writer_rx) = mpsc::channel(2);

    let mut connections = HashMap::new();
    connections.insert(
        normal_connection_id,
        initialized_connection(normal_writer_tx),
    );
    connections.insert(
        firehose_connection_id,
        initialized_firehose_connection(
            firehose_writer_tx,
            HashSet::from(["configWarning".to_string()]),
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::Broadcast {
            message: OutgoingMessage::AppServerNotification(config_warning("broadcast")),
        },
    )
    .await;

    let normal_message = normal_writer_rx
        .try_recv()
        .expect("normal connection should receive broadcast");
    assert!(matches!(
        normal_message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "broadcast"
    ));

    let firehose_message = firehose_writer_rx
        .try_recv()
        .expect("firehose connection should receive observed broadcast despite opt-out");
    assert!(matches!(
        firehose_message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "broadcast"
    ));
    assert!(
        firehose_writer_rx.try_recv().is_err(),
        "firehose connection should receive only one copy"
    );
}

#[tokio::test]
async fn targeted_notifications_emit_one_firehose_copy_per_logical_event() {
    let target_one = ConnectionId(31);
    let target_two = ConnectionId(32);
    let firehose_connection_id = ConnectionId(33);
    let (target_one_tx, mut target_one_rx) = mpsc::channel(2);
    let (target_two_tx, mut target_two_rx) = mpsc::channel(2);
    let (firehose_tx, mut firehose_rx) = mpsc::channel(2);

    let mut connections = HashMap::new();
    connections.insert(target_one, initialized_connection(target_one_tx));
    connections.insert(target_two, initialized_connection(target_two_tx));
    connections.insert(
        firehose_connection_id,
        initialized_firehose_connection(firehose_tx, HashSet::new()),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnections {
            connection_ids: vec![target_one, target_two],
            message: OutgoingMessage::AppServerNotification(config_warning("targeted")),
        },
    )
    .await;

    target_one_rx
        .try_recv()
        .expect("first target should receive notification");
    target_two_rx
        .try_recv()
        .expect("second target should receive notification");
    let firehose_message = firehose_rx
        .try_recv()
        .expect("firehose should receive observed notification");
    assert!(matches!(
        firehose_message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "targeted"
    ));
    assert!(
        firehose_rx.try_recv().is_err(),
        "firehose should receive one copy for the logical targeted notification"
    );
}

#[tokio::test]
async fn targeted_firehose_connection_receives_only_target_copy() {
    let target_firehose = ConnectionId(41);
    let (writer_tx, mut writer_rx) = mpsc::channel(2);

    let mut connections = HashMap::new();
    connections.insert(
        target_firehose,
        initialized_firehose_connection(writer_tx, HashSet::from(["configWarning".to_string()])),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnections {
            connection_ids: vec![target_firehose],
            message: OutgoingMessage::AppServerNotification(config_warning("target-firehose")),
        },
    )
    .await;

    writer_rx
        .try_recv()
        .expect("target firehose should receive target notification");
    assert!(
        writer_rx.try_recv().is_err(),
        "target firehose should not receive a second observed copy"
    );
}

#[tokio::test]
async fn server_requests_are_observed_without_becoming_answerable() {
    let target_connection_id = ConnectionId(51);
    let firehose_connection_id = ConnectionId(52);
    let (target_tx, mut target_rx) = mpsc::channel(2);
    let (firehose_tx, mut firehose_rx) = mpsc::channel(2);

    let target_state = initialized_connection(target_tx);
    let target_session = Arc::clone(&target_state.session);
    let firehose_state = initialized_firehose_connection(firehose_tx, HashSet::new());
    let firehose_session = Arc::clone(&firehose_state.session);

    let mut connections = HashMap::new();
    connections.insert(target_connection_id, target_state);
    connections.insert(firehose_connection_id, firehose_state);

    let request = ServerRequest::DynamicToolCall {
        request_id: RequestId::Integer(7),
        params: DynamicToolCallParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            namespace: None,
            tool: "tool".to_string(),
            arguments: json!({}),
        },
    };

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnections {
            connection_ids: vec![target_connection_id],
            message: OutgoingMessage::Request(request.clone()),
        },
    )
    .await;

    let target_message = target_rx
        .try_recv()
        .expect("target connection should receive request");
    assert!(matches!(
        target_message.message,
        OutgoingMessage::Request(ServerRequest::DynamicToolCall { .. })
    ));

    let firehose_message = firehose_rx
        .try_recv()
        .expect("firehose connection should receive observed request notification");
    assert!(matches!(
        firehose_message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ServerRequestObserved(
            ServerRequestObservedNotification { request: observed }
        )) if *observed == request
    ));
    assert!(
        target_session
            .take_answerable_server_request(&RequestId::Integer(7))
            .await
    );
    assert!(
        !firehose_session
            .take_answerable_server_request(&RequestId::Integer(7))
            .await
    );
}

#[tokio::test]
async fn experimental_notifications_are_dropped_without_capability() {
    let connection_id = ConnectionId(12);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(HashSet::new())),
            /*disconnect_sender*/ None,
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::AppServerNotification(thread_realtime_started_notification()),
            write_complete_tx: None,
        },
    )
    .await;

    assert!(
        writer_rx.try_recv().is_err(),
        "experimental notifications should not reach clients without capability"
    );
}

#[tokio::test]
async fn experimental_notifications_are_preserved_with_capability() {
    let connection_id = ConnectionId(13);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(RwLock::new(HashSet::new())),
            /*disconnect_sender*/ None,
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::AppServerNotification(thread_realtime_started_notification()),
            write_complete_tx: None,
        },
    )
    .await;

    let message = writer_rx
        .recv()
        .await
        .expect("experimental notification should reach opted-in client");
    assert!(matches!(
        message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ThreadRealtimeStarted(_))
    ));
}

#[tokio::test]
async fn command_execution_request_approval_strips_additional_permissions_without_capability() {
    let connection_id = ConnectionId(8);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(HashSet::new())),
            /*disconnect_sender*/ None,
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::Request(ServerRequest::CommandExecutionRequestApproval {
                request_id: RequestId::Integer(1),
                params: codex_app_server_protocol::CommandExecutionRequestApprovalParams {
                    thread_id: "thr_123".to_string(),
                    turn_id: "turn_123".to_string(),
                    item_id: "call_123".to_string(),
                    started_at_ms: 0,
                    approval_id: None,
                    environment_id: None,
                    reason: Some("Need extra read access".to_string()),
                    network_approval_context: None,
                    command: Some("cat file".to_string()),
                    cwd: Some(absolute_path("/tmp").into()),
                    command_actions: None,
                    additional_permissions: Some(
                        codex_app_server_protocol::AdditionalPermissionProfile {
                            network: None,
                            file_system: Some(
                                codex_app_server_protocol::AdditionalFileSystemPermissions {
                                    read: Some(vec![absolute_path("/tmp/allowed").into()]),
                                    write: None,
                                    glob_scan_max_depth: None,
                                    entries: None,
                                },
                            ),
                        },
                    ),
                    proposed_execpolicy_amendment: None,
                    proposed_network_policy_amendments: None,
                    available_decisions: None,
                },
            }),
            write_complete_tx: None,
        },
    )
    .await;

    let message = writer_rx
        .recv()
        .await
        .expect("request should be delivered to the connection");
    let json = serde_json::to_value(message.message).expect("request should serialize");
    assert_eq!(json["params"].get("additionalPermissions"), None);
}

#[tokio::test]
async fn command_execution_request_approval_keeps_additional_permissions_with_capability() {
    let connection_id = ConnectionId(9);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(RwLock::new(HashSet::new())),
            /*disconnect_sender*/ None,
        ),
    );

    route_outgoing_envelope(
        &mut connections,
        OutgoingEnvelope::ToConnection {
            connection_id,
            message: OutgoingMessage::Request(ServerRequest::CommandExecutionRequestApproval {
                request_id: RequestId::Integer(1),
                params: codex_app_server_protocol::CommandExecutionRequestApprovalParams {
                    thread_id: "thr_123".to_string(),
                    turn_id: "turn_123".to_string(),
                    item_id: "call_123".to_string(),
                    started_at_ms: 0,
                    approval_id: None,
                    environment_id: None,
                    reason: Some("Need extra read access".to_string()),
                    network_approval_context: None,
                    command: Some("cat file".to_string()),
                    cwd: Some(absolute_path("/tmp").into()),
                    command_actions: None,
                    additional_permissions: Some(
                        codex_app_server_protocol::AdditionalPermissionProfile {
                            network: None,
                            file_system: Some(
                                codex_app_server_protocol::AdditionalFileSystemPermissions {
                                    read: Some(vec![absolute_path("/tmp/allowed").into()]),
                                    write: None,
                                    glob_scan_max_depth: None,
                                    entries: None,
                                },
                            ),
                        },
                    ),
                    proposed_execpolicy_amendment: None,
                    proposed_network_policy_amendments: None,
                    available_decisions: None,
                },
            }),
            write_complete_tx: None,
        },
    )
    .await;

    let message = writer_rx
        .recv()
        .await
        .expect("request should be delivered to the connection");
    let json = serde_json::to_value(message.message).expect("request should serialize");
    let allowed_path = absolute_path("/tmp/allowed").to_string_lossy().into_owned();
    assert_eq!(
        json["params"]["additionalPermissions"],
        json!({
            "network": null,
            "fileSystem": {
                "read": [allowed_path],
            "write": null,
            },
        })
    );
}

#[tokio::test]
async fn broadcast_does_not_block_on_slow_connection() {
    let fast_connection_id = ConnectionId(1);
    let slow_connection_id = ConnectionId(2);

    let (fast_writer_tx, mut fast_writer_rx) = mpsc::channel(1);
    let (slow_writer_tx, mut slow_writer_rx) = mpsc::channel(1);
    let fast_disconnect_token = CancellationToken::new();
    let slow_disconnect_token = CancellationToken::new();

    let mut connections = HashMap::new();
    connections.insert(
        fast_connection_id,
        OutboundConnectionState::new(
            fast_writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(RwLock::new(HashSet::new())),
            Some(fast_disconnect_token.clone()),
        ),
    );
    connections.insert(
        slow_connection_id,
        OutboundConnectionState::new(
            slow_writer_tx.clone(),
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(RwLock::new(HashSet::new())),
            Some(slow_disconnect_token.clone()),
        ),
    );

    let queued_message = OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
        ConfigWarningNotification {
            summary: "already-buffered".to_string(),
            details: None,
            path: None,
            range: None,
        },
    ));
    slow_writer_tx
        .try_send(QueuedOutgoingMessage::new(queued_message))
        .expect("channel should have room");

    let broadcast_message = OutgoingMessage::AppServerNotification(
        ServerNotification::ConfigWarning(ConfigWarningNotification {
            summary: "test".to_string(),
            details: None,
            path: None,
            range: None,
        }),
    );
    timeout(
        Duration::from_millis(100),
        route_outgoing_envelope(
            &mut connections,
            OutgoingEnvelope::Broadcast {
                message: broadcast_message,
            },
        ),
    )
    .await
    .expect("broadcast should return even when one connection is slow");
    assert!(!connections.contains_key(&slow_connection_id));
    assert!(slow_disconnect_token.is_cancelled());
    assert!(!fast_disconnect_token.is_cancelled());
    let fast_message = fast_writer_rx
        .try_recv()
        .expect("fast connection should receive the broadcast notification");
    assert!(matches!(
        fast_message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "test"
    ));

    let slow_message = slow_writer_rx
        .try_recv()
        .expect("slow connection should retain its original buffered message");
    assert!(matches!(
        slow_message.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "already-buffered"
    ));
}

#[tokio::test]
async fn to_connection_stdio_waits_instead_of_disconnecting_when_writer_queue_is_full() {
    let connection_id = ConnectionId(3);
    let (writer_tx, mut writer_rx) = mpsc::channel(1);
    writer_tx
        .send(QueuedOutgoingMessage::new(
            OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
                ConfigWarningNotification {
                    summary: "queued".to_string(),
                    details: None,
                    path: None,
                    range: None,
                },
            )),
        ))
        .await
        .expect("channel should accept the first queued message");

    let mut connections = HashMap::new();
    connections.insert(
        connection_id,
        OutboundConnectionState::new(
            writer_tx,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(RwLock::new(HashSet::new())),
            /*disconnect_sender*/ None,
        ),
    );

    let route_task = tokio::spawn(async move {
        route_outgoing_envelope(
            &mut connections,
            OutgoingEnvelope::ToConnection {
                connection_id,
                message: OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
                    ConfigWarningNotification {
                        summary: "second".to_string(),
                        details: None,
                        path: None,
                        range: None,
                    },
                )),
                write_complete_tx: None,
            },
        )
        .await
    });

    let first = timeout(Duration::from_millis(100), writer_rx.recv())
        .await
        .expect("first queued message should be readable")
        .expect("first queued message should exist");
    timeout(Duration::from_millis(100), route_task)
        .await
        .expect("routing should finish after the first queued message is drained")
        .expect("routing task should succeed");

    assert!(matches!(
        first.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "queued"
    ));
    let second = writer_rx
        .try_recv()
        .expect("second notification should be delivered once the queue has room");
    assert!(matches!(
        second.message,
        OutgoingMessage::AppServerNotification(ServerNotification::ConfigWarning(
            ConfigWarningNotification { summary, .. }
        )) if summary == "second"
    ));
}
