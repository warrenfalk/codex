use std::collections::HashMap;
use std::path::PathBuf;

use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::CommandExecutionRequestApprovalResponse;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::FileChangeRequestApprovalParams;
use codex_app_server_protocol::FileChangeRequestApprovalResponse;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnStartParams;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::approvals::ExecApprovalRequestEvent;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::request_user_input::RequestUserInputEvent;
use codex_protocol::request_user_input::RequestUserInputQuestion as CoreRequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption as CoreRequestUserInputQuestionOption;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use futures::SinkExt;
use futures::StreamExt;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

type WsClient = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone, Debug)]
struct PendingRequest {
    kind: &'static str,
}

#[derive(Debug)]
struct ConnectedSessionState {
    current_turn_id: Option<String>,
    next_request_id: i64,
    pending_requests: HashMap<RequestId, PendingRequest>,
    pending_exec_approvals: HashMap<String, RequestId>,
    pending_patch_approvals: HashMap<String, RequestId>,
    pending_user_input_requests: HashMap<String, RequestId>,
    thread_id: String,
    thread_cwd: PathBuf,
}

impl ConnectedSessionState {
    fn new(thread_id: String, thread_cwd: PathBuf, next_request_id: i64) -> Self {
        Self {
            current_turn_id: None,
            next_request_id,
            pending_requests: HashMap::new(),
            pending_exec_approvals: HashMap::new(),
            pending_patch_approvals: HashMap::new(),
            pending_user_input_requests: HashMap::new(),
            thread_id,
            thread_cwd,
        }
    }

    fn allocate_request_id(&mut self, kind: &'static str) -> RequestId {
        let request_id = RequestId::Integer(self.next_request_id);
        self.next_request_id += 1;
        self.pending_requests
            .insert(request_id.clone(), PendingRequest { kind });
        request_id
    }
}

pub(crate) struct ConnectedSessionBootstrap {
    pub(crate) codex_op_tx: UnboundedSender<Op>,
    pub(crate) session_configured_event: Event,
}

pub(crate) async fn connect(
    url: &str,
    config: &Config,
    app_event_tx: AppEventSender,
) -> Result<ConnectedSessionBootstrap> {
    let (mut ws, _) = connect_async(url)
        .await
        .wrap_err_with(|| format!("failed to connect to app-server websocket at {url}"))?;

    let initialize_request_id = RequestId::Integer(1);
    send_request(
        &mut ws,
        "initialize",
        initialize_request_id.clone(),
        Some(serde_json::to_value(InitializeParams {
            client_info: ClientInfo {
                name: "codex-tui".to_string(),
                title: Some("Codex TUI".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            capabilities: Some(InitializeCapabilities {
                experimental_api: true,
                opt_out_notification_methods: None,
            }),
        })?),
    )
    .await?;
    read_response_for_id(&mut ws, &initialize_request_id).await?;
    send_notification(&mut ws, "initialized", Option::<serde_json::Value>::None).await?;

    let thread_start_request_id = RequestId::Integer(2);
    let thread_start_params = ThreadStartParams {
        model: config.model.clone(),
        model_provider: None,
        service_tier: None,
        cwd: Some(config.cwd.display().to_string()),
        approval_policy: Some(config.permissions.approval_policy.value().into()),
        approvals_reviewer: None,
        sandbox: None,
        config: None,
        service_name: None,
        base_instructions: None,
        developer_instructions: config.developer_instructions.clone(),
        personality: config.personality,
        ephemeral: None,
        dynamic_tools: None,
        mock_experimental_field: None,
        experimental_raw_events: false,
        persist_extended_history: false,
    };
    send_request(
        &mut ws,
        "thread/start",
        thread_start_request_id.clone(),
        Some(serde_json::to_value(thread_start_params)?),
    )
    .await?;
    let thread_start_response = read_response_for_id(&mut ws, &thread_start_request_id).await?;
    let thread_start: ThreadStartResponse = serde_json::from_value(thread_start_response.result)
        .wrap_err("decode thread/start response")?;

    let thread_id = ThreadId::try_from(thread_start.thread.id.as_str())
        .wrap_err("invalid thread id in thread/start response")?;
    let thread_cwd = thread_start.cwd.clone();
    let session_configured_event = Event {
        id: String::new(),
        msg: EventMsg::SessionConfigured(SessionConfiguredEvent {
            session_id: thread_id,
            forked_from_id: None,
            thread_name: None,
            model: thread_start.model,
            model_provider_id: thread_start.model_provider,
            service_tier: thread_start.service_tier,
            approval_policy: thread_start.approval_policy.to_core(),
            approvals_reviewer: thread_start.approvals_reviewer.to_core(),
            sandbox_policy: thread_start.sandbox.to_core(),
            cwd: thread_start.cwd,
            reasoning_effort: thread_start.reasoning_effort,
            history_log_id: 0,
            history_entry_count: 0,
            initial_messages: None,
            network_proxy: None,
            rollout_path: None,
        }),
    };

    let state = std::sync::Arc::new(Mutex::new(ConnectedSessionState::new(
        thread_id.to_string(),
        thread_cwd,
        3,
    )));
    let (outbound_tx, outbound_rx) = unbounded_channel::<JSONRPCMessage>();
    let (codex_op_tx, codex_op_rx) = unbounded_channel::<Op>();
    let (write, read) = ws.split();

    tokio::spawn(writer_task(write, outbound_rx, app_event_tx.clone()));
    tokio::spawn(reader_task(
        read,
        outbound_tx.clone(),
        state.clone(),
        app_event_tx.clone(),
    ));
    tokio::spawn(op_task(
        codex_op_rx,
        outbound_tx,
        state,
        app_event_tx.clone(),
    ));

    Ok(ConnectedSessionBootstrap {
        codex_op_tx,
        session_configured_event,
    })
}

async fn writer_task<S>(
    mut write: S,
    mut outbound_rx: UnboundedReceiver<JSONRPCMessage>,
    app_event_tx: AppEventSender,
) where
    S: futures::Sink<WebSocketMessage, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin
        + Send
        + 'static,
{
    while let Some(message) = outbound_rx.recv().await {
        let payload = match serde_json::to_string(&message) {
            Ok(payload) => payload,
            Err(err) => {
                send_error_event(
                    &app_event_tx,
                    format!("failed to serialize app-server message: {err}"),
                );
                continue;
            }
        };
        if let Err(err) = write.send(WebSocketMessage::Text(payload.into())).await {
            app_event_tx.send(AppEvent::FatalExitRequest(format!(
                "app-server websocket send failed: {err}"
            )));
            break;
        }
    }
}

async fn reader_task<S>(
    mut read: S,
    outbound_tx: UnboundedSender<JSONRPCMessage>,
    state: std::sync::Arc<Mutex<ConnectedSessionState>>,
    app_event_tx: AppEventSender,
) where
    S: futures::Stream<
            Item = std::result::Result<WebSocketMessage, tokio_tungstenite::tungstenite::Error>,
        > + Unpin
        + Send
        + 'static,
{
    loop {
        let frame = match read.next().await {
            Some(Ok(frame)) => frame,
            Some(Err(err)) => {
                app_event_tx.send(AppEvent::FatalExitRequest(format!(
                    "app-server websocket read failed: {err}"
                )));
                break;
            }
            None => {
                app_event_tx.send(AppEvent::FatalExitRequest(
                    "app-server websocket connection closed".to_string(),
                ));
                break;
            }
        };

        let Some(message) = parse_websocket_message(frame, &app_event_tx) else {
            continue;
        };

        match message {
            JSONRPCMessage::Notification(notification) => {
                handle_notification(notification, &state, &app_event_tx).await;
            }
            JSONRPCMessage::Response(response) => {
                let mut guard = state.lock().await;
                guard.pending_requests.remove(&response.id);
            }
            JSONRPCMessage::Error(err) => {
                let mut guard = state.lock().await;
                let pending = guard.pending_requests.remove(&err.id);
                let request_kind = pending.as_ref().map_or("request", |pending| pending.kind);
                send_error_event(
                    &app_event_tx,
                    format!("app-server {request_kind} failed: {}", err.error.message),
                );
            }
            JSONRPCMessage::Request(request) => {
                handle_server_request(request, &outbound_tx, &state, &app_event_tx).await;
            }
        }
    }
}

async fn op_task(
    mut codex_op_rx: UnboundedReceiver<Op>,
    outbound_tx: UnboundedSender<JSONRPCMessage>,
    state: std::sync::Arc<Mutex<ConnectedSessionState>>,
    app_event_tx: AppEventSender,
) {
    while let Some(op) = codex_op_rx.recv().await {
        match op {
            Op::UserTurn {
                items,
                cwd,
                approval_policy,
                sandbox_policy,
                model,
                effort,
                personality,
                final_output_json_schema,
                ..
            } => {
                let request_id = {
                    let mut guard = state.lock().await;
                    let thread_id = guard.thread_id.clone();
                    guard.thread_cwd = PathBuf::from(&cwd);
                    let request_id = guard.allocate_request_id("turn/start");
                    let request = JSONRPCMessage::Request(JSONRPCRequest {
                        id: request_id.clone(),
                        method: "turn/start".to_string(),
                        params: Some(
                            serde_json::to_value(TurnStartParams {
                                thread_id,
                                input: items.into_iter().map(Into::into).collect(),
                                cwd: Some(cwd),
                                approval_policy: Some(approval_policy.into()),
                                approvals_reviewer: None,
                                sandbox_policy: Some(sandbox_policy.into()),
                                model: Some(model),
                                service_tier: None,
                                effort,
                                summary: None,
                                personality,
                                output_schema: final_output_json_schema,
                                collaboration_mode: None,
                            })
                            .unwrap_or_default(),
                        ),
                        trace: None,
                    });
                    if outbound_tx.send(request).is_err() {
                        send_error_event(
                            &app_event_tx,
                            "failed to send turn/start to app-server".to_string(),
                        );
                    }
                    request_id
                };
                let _ = request_id;
            }
            Op::Interrupt => {
                let maybe_request = {
                    let mut guard = state.lock().await;
                    guard.current_turn_id.clone().map(|turn_id| {
                        let request_id = guard.allocate_request_id("turn/interrupt");
                        JSONRPCMessage::Request(JSONRPCRequest {
                            id: request_id,
                            method: "turn/interrupt".to_string(),
                            params: Some(
                                serde_json::to_value(TurnInterruptParams {
                                    thread_id: guard.thread_id.clone(),
                                    turn_id,
                                })
                                .unwrap_or_default(),
                            ),
                            trace: None,
                        })
                    })
                };
                if let Some(request) = maybe_request
                    && outbound_tx.send(request).is_err()
                {
                    send_error_event(
                        &app_event_tx,
                        "failed to send turn/interrupt to app-server".to_string(),
                    );
                }
            }
            Op::Shutdown => {
                app_event_tx.send(AppEvent::CodexEvent(Event {
                    id: String::new(),
                    msg: EventMsg::ShutdownComplete,
                }));
            }
            Op::ExecApproval { id, decision, .. } => {
                let request_id = {
                    let mut guard = state.lock().await;
                    guard.pending_exec_approvals.remove(&id)
                };
                let Some(request_id) = request_id else {
                    send_warning_event(
                        &app_event_tx,
                        format!("no pending connected exec approval found for `{id}`"),
                    );
                    continue;
                };
                let response = JSONRPCMessage::Response(JSONRPCResponse {
                    id: request_id,
                    result: serde_json::to_value(CommandExecutionRequestApprovalResponse {
                        decision: review_decision_to_command_execution_approval_decision(decision),
                    })
                    .unwrap_or_default(),
                });
                if outbound_tx.send(response).is_err() {
                    send_error_event(
                        &app_event_tx,
                        "failed to send exec approval response to app-server".to_string(),
                    );
                }
            }
            Op::PatchApproval { id, decision } => {
                let request_id = {
                    let mut guard = state.lock().await;
                    guard.pending_patch_approvals.remove(&id)
                };
                let Some(request_id) = request_id else {
                    send_warning_event(
                        &app_event_tx,
                        format!("no pending connected patch approval found for `{id}`"),
                    );
                    continue;
                };
                let response = JSONRPCMessage::Response(JSONRPCResponse {
                    id: request_id,
                    result: serde_json::to_value(FileChangeRequestApprovalResponse {
                        decision: review_decision_to_file_change_approval_decision(decision),
                    })
                    .unwrap_or_default(),
                });
                if outbound_tx.send(response).is_err() {
                    send_error_event(
                        &app_event_tx,
                        "failed to send patch approval response to app-server".to_string(),
                    );
                }
            }
            Op::UserInputAnswer { id, response } => {
                let request_id = {
                    let mut guard = state.lock().await;
                    guard.pending_user_input_requests.remove(&id)
                };
                let Some(request_id) = request_id else {
                    send_warning_event(
                        &app_event_tx,
                        format!("no pending connected request_user_input found for `{id}`"),
                    );
                    continue;
                };
                let response = JSONRPCMessage::Response(JSONRPCResponse {
                    id: request_id,
                    result: serde_json::to_value(ToolRequestUserInputResponse {
                        answers: response
                            .answers
                            .into_iter()
                            .map(|(question_id, answer)| {
                                (
                                    question_id,
                                    ToolRequestUserInputAnswer {
                                        answers: answer.answers,
                                    },
                                )
                            })
                            .collect(),
                    })
                    .unwrap_or_default(),
                });
                if outbound_tx.send(response).is_err() {
                    send_error_event(
                        &app_event_tx,
                        "failed to send request_user_input response to app-server".to_string(),
                    );
                }
            }
            Op::AddToHistory { .. }
            | Op::ListCustomPrompts
            | Op::ListSkills { .. }
            | Op::OverrideTurnContext { .. }
            | Op::ReloadUserConfig => {}
            _ => {
                send_warning_event(
                    &app_event_tx,
                    "connected mode POC ignores this action".to_string(),
                );
            }
        }
    }
}

async fn send_request(
    ws: &mut WsClient,
    method: &str,
    id: RequestId,
    params: Option<serde_json::Value>,
) -> Result<()> {
    let message = JSONRPCMessage::Request(JSONRPCRequest {
        id,
        method: method.to_string(),
        params,
        trace: None,
    });
    let payload = serde_json::to_string(&message)?;
    ws.send(WebSocketMessage::Text(payload.into()))
        .await
        .wrap_err("failed to send websocket frame")
}

async fn send_notification(
    ws: &mut WsClient,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<()> {
    let message = JSONRPCMessage::Notification(JSONRPCNotification {
        method: method.to_string(),
        params,
    });
    let payload = serde_json::to_string(&message)?;
    ws.send(WebSocketMessage::Text(payload.into()))
        .await
        .wrap_err("failed to send websocket frame")
}

async fn read_response_for_id(
    ws: &mut WsClient,
    request_id: &RequestId,
) -> Result<JSONRPCResponse> {
    loop {
        match read_jsonrpc_message(ws).await? {
            JSONRPCMessage::Response(response) if response.id == *request_id => {
                return Ok(response);
            }
            JSONRPCMessage::Error(JSONRPCError { error, id }) if id == *request_id => {
                color_eyre::eyre::bail!("app-server request failed: {}", error.message);
            }
            _ => {}
        }
    }
}

async fn read_jsonrpc_message(ws: &mut WsClient) -> Result<JSONRPCMessage> {
    loop {
        let frame = match ws.next().await {
            Some(frame) => frame.wrap_err("failed to read websocket frame")?,
            None => {
                return Err(color_eyre::eyre::eyre!(
                    "websocket stream ended unexpectedly"
                ));
            }
        };
        if let Some(message) = parse_websocket_message(frame, &AppEventSender::noop()) {
            return Ok(message);
        }
    }
}

fn parse_websocket_message(
    frame: WebSocketMessage,
    app_event_tx: &AppEventSender,
) -> Option<JSONRPCMessage> {
    match frame {
        WebSocketMessage::Text(text) => match serde_json::from_str(text.as_ref()) {
            Ok(message) => Some(message),
            Err(err) => {
                send_error_event(
                    app_event_tx,
                    format!("failed to decode app-server message: {err}"),
                );
                None
            }
        },
        WebSocketMessage::Binary(_) => {
            send_warning_event(
                app_event_tx,
                "ignoring unexpected binary websocket frame from app-server".to_string(),
            );
            None
        }
        WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) | WebSocketMessage::Frame(_) => None,
        WebSocketMessage::Close(_) => None,
    }
}

async fn handle_notification(
    notification: JSONRPCNotification,
    state: &std::sync::Arc<Mutex<ConnectedSessionState>>,
    app_event_tx: &AppEventSender,
) {
    if !notification.method.starts_with("codex/event/") {
        return;
    }
    let Some(params) = notification.params else {
        return;
    };
    let Ok(event) = serde_json::from_value::<Event>(params) else {
        send_error_event(
            app_event_tx,
            format!(
                "failed to decode legacy event notification `{}`",
                notification.method
            ),
        );
        return;
    };

    {
        let mut guard = state.lock().await;
        match &event.msg {
            // In connected mode, app-server sends these as server requests as well as
            // legacy codex/event notifications. The request path is the only one that
            // carries a JSON-RPC callback id, so processing the legacy event here
            // duplicates the prompt and leaves the second approval unresolved.
            EventMsg::ExecApprovalRequest(_)
            | EventMsg::ApplyPatchApprovalRequest(_)
            | EventMsg::RequestUserInput(_) => {
                return;
            }
            EventMsg::TurnStarted(turn_started) => {
                guard.current_turn_id = Some(turn_started.turn_id.clone());
            }
            EventMsg::TurnComplete(turn_complete) => {
                if guard.current_turn_id.as_deref() == Some(turn_complete.turn_id.as_str()) {
                    guard.current_turn_id = None;
                }
            }
            EventMsg::TurnAborted(turn_aborted) => {
                if guard.current_turn_id.as_deref() == turn_aborted.turn_id.as_deref() {
                    guard.current_turn_id = None;
                }
            }
            EventMsg::ShutdownComplete => {
                guard.current_turn_id = None;
            }
            _ => {}
        }
    }

    app_event_tx.send(AppEvent::CodexEvent(event));
}

async fn handle_server_request(
    request: JSONRPCRequest,
    outbound_tx: &UnboundedSender<JSONRPCMessage>,
    state: &std::sync::Arc<Mutex<ConnectedSessionState>>,
    app_event_tx: &AppEventSender,
) {
    match request.method.as_str() {
        "item/commandExecution/requestApproval" => {
            let Some(params) = request.params else {
                send_warning_event(
                    app_event_tx,
                    "connected mode received exec approval request without params".to_string(),
                );
                send_unsupported_request_response(
                    outbound_tx,
                    request.id,
                    "exec approval request missing params",
                    app_event_tx,
                );
                return;
            };
            let approval =
                match serde_json::from_value::<CommandExecutionRequestApprovalParams>(params) {
                    Ok(approval) => approval,
                    Err(err) => {
                        send_error_event(
                            app_event_tx,
                            format!("failed to decode exec approval request: {err}"),
                        );
                        send_unsupported_request_response(
                            outbound_tx,
                            request.id,
                            "failed to decode exec approval request",
                            app_event_tx,
                        );
                        return;
                    }
                };
            let approval_key = approval
                .approval_id
                .clone()
                .unwrap_or_else(|| approval.item_id.clone());
            let cwd = {
                let mut guard = state.lock().await;
                guard
                    .pending_exec_approvals
                    .insert(approval_key.clone(), request.id.clone());
                guard.thread_cwd.clone()
            };
            let event = Event {
                id: String::new(),
                msg: EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
                    call_id: approval.item_id,
                    approval_id: approval.approval_id,
                    turn_id: approval.turn_id,
                    command: approval
                        .command
                        .map_or_else(Vec::new, |command| vec![command]),
                    cwd: approval.cwd.unwrap_or(cwd),
                    reason: approval.reason,
                    network_approval_context: approval
                        .network_approval_context
                        .map(network_approval_context_to_core),
                    proposed_execpolicy_amendment: approval
                        .proposed_execpolicy_amendment
                        .map(codex_app_server_protocol::ExecPolicyAmendment::into_core),
                    proposed_network_policy_amendments: approval
                        .proposed_network_policy_amendments
                        .map(|amendments| {
                            amendments
                                .into_iter()
                                .map(codex_app_server_protocol::NetworkPolicyAmendment::into_core)
                                .collect()
                        }),
                    additional_permissions: approval
                        .additional_permissions
                        .map(additional_permission_profile_to_core),
                    skill_metadata: approval.skill_metadata.map(|skill_metadata| {
                        codex_protocol::approvals::ExecApprovalRequestSkillMetadata {
                            path_to_skills_md: skill_metadata.path_to_skills_md,
                        }
                    }),
                    available_decisions: approval.available_decisions.map(|decisions| {
                        decisions
                            .into_iter()
                            .map(command_execution_approval_decision_to_review_decision)
                            .collect()
                    }),
                    parsed_cmd: Vec::new(),
                }),
            };
            app_event_tx.send(AppEvent::CodexEvent(event));
        }
        "item/fileChange/requestApproval" => {
            let Some(params) = request.params else {
                send_warning_event(
                    app_event_tx,
                    "connected mode received file change approval request without params"
                        .to_string(),
                );
                send_unsupported_request_response(
                    outbound_tx,
                    request.id,
                    "file change approval request missing params",
                    app_event_tx,
                );
                return;
            };
            let approval = match serde_json::from_value::<FileChangeRequestApprovalParams>(params) {
                Ok(approval) => approval,
                Err(err) => {
                    send_error_event(
                        app_event_tx,
                        format!("failed to decode file change approval request: {err}"),
                    );
                    send_unsupported_request_response(
                        outbound_tx,
                        request.id,
                        "failed to decode file change approval request",
                        app_event_tx,
                    );
                    return;
                }
            };
            {
                let mut guard = state.lock().await;
                guard
                    .pending_patch_approvals
                    .insert(approval.item_id.clone(), request.id.clone());
            }
            let event = Event {
                id: String::new(),
                msg: EventMsg::ApplyPatchApprovalRequest(
                    codex_protocol::protocol::ApplyPatchApprovalRequestEvent {
                        call_id: approval.item_id,
                        turn_id: approval.turn_id,
                        changes: HashMap::new(),
                        reason: approval.reason,
                        grant_root: approval.grant_root,
                    },
                ),
            };
            app_event_tx.send(AppEvent::CodexEvent(event));
        }
        "item/tool/requestUserInput" => {
            let Some(params) = request.params else {
                send_warning_event(
                    app_event_tx,
                    "connected mode received request_user_input request without params".to_string(),
                );
                send_unsupported_request_response(
                    outbound_tx,
                    request.id,
                    "request_user_input request missing params",
                    app_event_tx,
                );
                return;
            };
            let request_user_input =
                match serde_json::from_value::<ToolRequestUserInputParams>(params) {
                    Ok(request_user_input) => request_user_input,
                    Err(err) => {
                        send_error_event(
                            app_event_tx,
                            format!("failed to decode request_user_input request: {err}"),
                        );
                        send_unsupported_request_response(
                            outbound_tx,
                            request.id,
                            "failed to decode request_user_input request",
                            app_event_tx,
                        );
                        return;
                    }
                };
            {
                let mut guard = state.lock().await;
                guard
                    .pending_user_input_requests
                    .insert(request_user_input.turn_id.clone(), request.id.clone());
            }
            let event = Event {
                id: String::new(),
                msg: EventMsg::RequestUserInput(RequestUserInputEvent {
                    call_id: request_user_input.item_id,
                    turn_id: request_user_input.turn_id,
                    questions: request_user_input
                        .questions
                        .into_iter()
                        .map(tool_request_user_input_question_to_core)
                        .collect(),
                }),
            };
            app_event_tx.send(AppEvent::CodexEvent(event));
        }
        _ => {
            send_warning_event(
                app_event_tx,
                format!(
                    "connected mode received unsupported server request `{}`",
                    request.method
                ),
            );
            send_unsupported_request_response(
                outbound_tx,
                request.id,
                format!("connected mode does not support `{}`", request.method),
                app_event_tx,
            );
        }
    }
}

fn send_error_event(app_event_tx: &AppEventSender, message: String) {
    app_event_tx.send(AppEvent::CodexEvent(Event {
        id: String::new(),
        msg: EventMsg::Error(ErrorEvent {
            message,
            codex_error_info: None,
        }),
    }));
}

fn send_warning_event(app_event_tx: &AppEventSender, message: String) {
    app_event_tx.send(AppEvent::CodexEvent(Event {
        id: String::new(),
        msg: EventMsg::Warning(WarningEvent { message }),
    }));
}

trait AppEventSenderExt {
    fn noop() -> Self;
}

impl AppEventSenderExt for AppEventSender {
    fn noop() -> Self {
        let (tx, _rx) = unbounded_channel();
        Self::new(tx)
    }
}

fn send_unsupported_request_response(
    outbound_tx: &UnboundedSender<JSONRPCMessage>,
    id: RequestId,
    message: impl Into<String>,
    app_event_tx: &AppEventSender,
) {
    let response = JSONRPCMessage::Error(JSONRPCError {
        error: codex_app_server_protocol::JSONRPCErrorError {
            code: -32601,
            data: None,
            message: message.into(),
        },
        id,
    });
    if outbound_tx.send(response).is_err() {
        send_error_event(
            app_event_tx,
            "failed to send unsupported-request response to app-server".to_string(),
        );
    }
}

fn network_approval_context_to_core(
    value: codex_app_server_protocol::NetworkApprovalContext,
) -> codex_protocol::approvals::NetworkApprovalContext {
    codex_protocol::approvals::NetworkApprovalContext {
        host: value.host,
        protocol: match value.protocol {
            codex_app_server_protocol::NetworkApprovalProtocol::Http => {
                codex_protocol::approvals::NetworkApprovalProtocol::Http
            }
            codex_app_server_protocol::NetworkApprovalProtocol::Https => {
                codex_protocol::approvals::NetworkApprovalProtocol::Https
            }
            codex_app_server_protocol::NetworkApprovalProtocol::Socks5Tcp => {
                codex_protocol::approvals::NetworkApprovalProtocol::Socks5Tcp
            }
            codex_app_server_protocol::NetworkApprovalProtocol::Socks5Udp => {
                codex_protocol::approvals::NetworkApprovalProtocol::Socks5Udp
            }
        },
    }
}

fn additional_permission_profile_to_core(
    value: codex_app_server_protocol::AdditionalPermissionProfile,
) -> codex_protocol::models::PermissionProfile {
    codex_protocol::models::PermissionProfile {
        network: value.network.map(Into::into),
        file_system: value.file_system.map(|permissions| {
            codex_protocol::models::FileSystemPermissions {
                read: permissions.read,
                write: permissions.write,
            }
        }),
        macos: value.macos.map(|permissions| {
            codex_protocol::models::MacOsSeatbeltProfileExtensions {
                macos_preferences: permissions.preferences,
                macos_automation: permissions.automations,
                macos_launch_services: permissions.launch_services,
                macos_accessibility: permissions.accessibility,
                macos_calendar: permissions.calendar,
                macos_reminders: permissions.reminders,
                macos_contacts: permissions.contacts,
            }
        }),
    }
}

fn tool_request_user_input_question_to_core(
    value: ToolRequestUserInputQuestion,
) -> CoreRequestUserInputQuestion {
    CoreRequestUserInputQuestion {
        id: value.id,
        header: value.header,
        question: value.question,
        is_other: value.is_other,
        is_secret: value.is_secret,
        options: value.options.map(|options| {
            options
                .into_iter()
                .map(tool_request_user_input_question_option_to_core)
                .collect()
        }),
    }
}

fn tool_request_user_input_question_option_to_core(
    value: ToolRequestUserInputOption,
) -> CoreRequestUserInputQuestionOption {
    CoreRequestUserInputQuestionOption {
        label: value.label,
        description: value.description,
    }
}

fn command_execution_approval_decision_to_review_decision(
    value: CommandExecutionApprovalDecision,
) -> ReviewDecision {
    match value {
        CommandExecutionApprovalDecision::Accept => ReviewDecision::Approved,
        CommandExecutionApprovalDecision::AcceptForSession => ReviewDecision::ApprovedForSession,
        CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
            execpolicy_amendment,
        } => ReviewDecision::ApprovedExecpolicyAmendment {
            proposed_execpolicy_amendment: execpolicy_amendment.into_core(),
        },
        CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment,
        } => ReviewDecision::NetworkPolicyAmendment {
            network_policy_amendment: network_policy_amendment.into_core(),
        },
        CommandExecutionApprovalDecision::Decline => ReviewDecision::Denied,
        CommandExecutionApprovalDecision::Cancel => ReviewDecision::Abort,
    }
}

fn review_decision_to_command_execution_approval_decision(
    value: ReviewDecision,
) -> CommandExecutionApprovalDecision {
    match value {
        ReviewDecision::Approved => CommandExecutionApprovalDecision::Accept,
        ReviewDecision::ApprovedExecpolicyAmendment {
            proposed_execpolicy_amendment,
        } => CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
            execpolicy_amendment: proposed_execpolicy_amendment.into(),
        },
        ReviewDecision::ApprovedForSession => CommandExecutionApprovalDecision::AcceptForSession,
        ReviewDecision::NetworkPolicyAmendment {
            network_policy_amendment,
        } => CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment: network_policy_amendment.into(),
        },
        ReviewDecision::Denied => CommandExecutionApprovalDecision::Decline,
        ReviewDecision::Abort => CommandExecutionApprovalDecision::Cancel,
    }
}

fn review_decision_to_file_change_approval_decision(
    value: ReviewDecision,
) -> FileChangeApprovalDecision {
    match value {
        ReviewDecision::Approved => FileChangeApprovalDecision::Accept,
        ReviewDecision::ApprovedForSession => FileChangeApprovalDecision::AcceptForSession,
        ReviewDecision::ApprovedExecpolicyAmendment { .. }
        | ReviewDecision::NetworkPolicyAmendment { .. }
        | ReviewDecision::Denied => FileChangeApprovalDecision::Decline,
        ReviewDecision::Abort => FileChangeApprovalDecision::Cancel,
    }
}
