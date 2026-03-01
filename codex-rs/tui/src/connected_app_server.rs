use std::collections::HashMap;
use std::path::Path;
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
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSortKey as RemoteThreadSortKey;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_core::ThreadSortKey;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::approvals::ExecApprovalRequestEvent;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::ContextCompactionItem;
use codex_protocol::items::ReasoningItem;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::items::WebSearchItem;
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

pub(crate) enum ConnectedSessionMode {
    StartFresh,
    Resume { thread_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteThreadListPage {
    pub(crate) threads: Vec<RemoteThreadSummary>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteThreadSummary {
    pub(crate) id: String,
    pub(crate) preview: String,
    pub(crate) name: Option<String>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) cwd: PathBuf,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) git_branch: Option<String>,
}

pub(crate) async fn connect(
    url: &str,
    config: &Config,
    app_event_tx: AppEventSender,
    mode: ConnectedSessionMode,
) -> Result<ConnectedSessionBootstrap> {
    let (mut ws, _) = connect_async(url)
        .await
        .wrap_err_with(|| format!("failed to connect to app-server websocket at {url}"))?;

    initialize_connection(&mut ws).await?;
    let (thread_id, thread_cwd, session_configured_event) =
        open_thread_for_session(&mut ws, config, mode).await?;

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

pub(crate) async fn find_latest_thread_id(url: &str, cwd: Option<&Path>) -> Result<Option<String>> {
    Ok(
        list_threads_page(url, None, 1, ThreadSortKey::UpdatedAt, None, cwd)
            .await?
            .threads
            .into_iter()
            .next()
            .map(|thread| thread.id),
    )
}

pub(crate) async fn list_threads_page(
    url: &str,
    cursor: Option<String>,
    limit: u32,
    sort_key: ThreadSortKey,
    model_provider: Option<&str>,
    cwd: Option<&Path>,
) -> Result<RemoteThreadListPage> {
    let (mut ws, _) = connect_async(url)
        .await
        .wrap_err_with(|| format!("failed to connect to app-server websocket at {url}"))?;
    initialize_connection(&mut ws).await?;

    let request_id = RequestId::Integer(2);
    send_request(
        &mut ws,
        "thread/list",
        request_id.clone(),
        Some(serde_json::to_value(ThreadListParams {
            cursor,
            limit: Some(limit),
            sort_key: Some(remote_thread_sort_key(sort_key)),
            model_providers: model_provider.map(|provider| vec![provider.to_string()]),
            source_kinds: None,
            archived: None,
            cwd: cwd.map(|path| path.display().to_string()),
            search_term: None,
        })?),
    )
    .await?;
    let response = read_response_for_id(&mut ws, &request_id).await?;
    let threads: ThreadListResponse =
        serde_json::from_value(response.result).wrap_err("decode thread/list response")?;

    Ok(RemoteThreadListPage {
        threads: threads
            .data
            .into_iter()
            .map(|thread| RemoteThreadSummary {
                id: thread.id,
                preview: thread.preview,
                name: thread.name,
                path: thread.path,
                cwd: thread.cwd,
                created_at: thread.created_at,
                updated_at: thread.updated_at,
                git_branch: thread.git_info.and_then(|info| info.branch),
            })
            .collect(),
        next_cursor: threads.next_cursor,
    })
}

fn remote_thread_sort_key(sort_key: ThreadSortKey) -> RemoteThreadSortKey {
    match sort_key {
        ThreadSortKey::CreatedAt => RemoteThreadSortKey::CreatedAt,
        ThreadSortKey::UpdatedAt => RemoteThreadSortKey::UpdatedAt,
    }
}

async fn initialize_connection(ws: &mut WsClient) -> Result<()> {
    let initialize_request_id = RequestId::Integer(1);
    send_request(
        ws,
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
    read_response_for_id(ws, &initialize_request_id).await?;
    send_notification(ws, "initialized", Option::<serde_json::Value>::None).await
}

async fn open_thread_for_session(
    ws: &mut WsClient,
    config: &Config,
    mode: ConnectedSessionMode,
) -> Result<(ThreadId, PathBuf, Event)> {
    match mode {
        ConnectedSessionMode::StartFresh => {
            let request_id = RequestId::Integer(2);
            send_request(
                ws,
                "thread/start",
                request_id.clone(),
                Some(serde_json::to_value(ThreadStartParams {
                    model: config.model.clone(),
                    model_provider: None,
                    cwd: Some(config.cwd.display().to_string()),
                    approval_policy: Some(config.permissions.approval_policy.value().into()),
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
                })?),
            )
            .await?;
            let response = read_response_for_id(ws, &request_id).await?;
            let thread_start: ThreadStartResponse =
                serde_json::from_value(response.result).wrap_err("decode thread/start response")?;
            let thread_id = ThreadId::try_from(thread_start.thread.id.as_str())
                .wrap_err("invalid thread id in thread/start response")?;
            let thread_cwd = thread_start.cwd.clone();
            let session_configured_event = session_configured_event_from_thread_response(
                thread_start.thread,
                thread_start.model,
                thread_start.model_provider,
                thread_start.cwd,
                thread_start.approval_policy.to_core(),
                thread_start.sandbox.to_core(),
                thread_start.reasoning_effort,
            )
            .wrap_err("build session_configured event from thread/start response")?;
            Ok((thread_id, thread_cwd, session_configured_event))
        }
        ConnectedSessionMode::Resume { thread_id } => {
            let request_id = RequestId::Integer(2);
            send_request(
                ws,
                "thread/resume",
                request_id.clone(),
                Some(serde_json::to_value(ThreadResumeParams {
                    thread_id,
                    history: None,
                    path: None,
                    model: config.model.clone(),
                    model_provider: None,
                    cwd: Some(config.cwd.display().to_string()),
                    approval_policy: Some(config.permissions.approval_policy.value().into()),
                    sandbox: None,
                    config: None,
                    base_instructions: None,
                    developer_instructions: config.developer_instructions.clone(),
                    personality: config.personality,
                    persist_extended_history: false,
                })?),
            )
            .await?;
            let response = read_response_for_id(ws, &request_id).await?;
            let thread_resume: ThreadResumeResponse = serde_json::from_value(response.result)
                .wrap_err("decode thread/resume response")?;
            let thread_id = ThreadId::try_from(thread_resume.thread.id.as_str())
                .wrap_err("invalid thread id in thread/resume response")?;
            let thread_cwd = thread_resume.cwd.clone();
            let session_configured_event = session_configured_event_from_thread_response(
                thread_resume.thread,
                thread_resume.model,
                thread_resume.model_provider,
                thread_resume.cwd,
                thread_resume.approval_policy.to_core(),
                thread_resume.sandbox.to_core(),
                thread_resume.reasoning_effort,
            )
            .wrap_err("build session_configured event from thread/resume response")?;
            Ok((thread_id, thread_cwd, session_configured_event))
        }
    }
}

fn session_configured_event_from_thread_response(
    thread: Thread,
    model: String,
    model_provider_id: String,
    cwd: PathBuf,
    approval_policy: codex_protocol::protocol::AskForApproval,
    sandbox_policy: codex_protocol::protocol::SandboxPolicy,
    reasoning_effort: Option<codex_protocol::openai_models::ReasoningEffort>,
) -> Result<Event> {
    let session_id = ThreadId::try_from(thread.id.as_str())
        .wrap_err("invalid thread id while building session_configured event")?;
    let thread_cwd = cwd;
    Ok(Event {
        id: String::new(),
        msg: EventMsg::SessionConfigured(SessionConfiguredEvent {
            session_id,
            forked_from_id: None,
            thread_name: thread.name,
            model,
            model_provider_id,
            approval_policy,
            sandbox_policy,
            cwd: thread_cwd,
            reasoning_effort,
            history_log_id: 0,
            history_entry_count: 0,
            initial_messages: synthesize_initial_messages_from_turns(&thread.turns),
            network_proxy: None,
            rollout_path: thread.path,
        }),
    })
}

fn synthesize_initial_messages_from_turns(turns: &[Turn]) -> Option<Vec<EventMsg>> {
    let events = turns
        .iter()
        .flat_map(synthesize_initial_messages_from_turn)
        .collect::<Vec<_>>();
    (!events.is_empty()).then_some(events)
}

fn synthesize_initial_messages_from_turn(turn: &Turn) -> Vec<EventMsg> {
    if matches!(turn.status, TurnStatus::InProgress) {
        return Vec::new();
    }

    turn.items
        .iter()
        .flat_map(synthesize_initial_messages_from_item)
        .collect()
}

fn synthesize_initial_messages_from_item(item: &ThreadItem) -> Vec<EventMsg> {
    match item {
        ThreadItem::UserMessage { id, content } => TurnItem::UserMessage(UserMessageItem {
            id: id.clone(),
            content: content
                .clone()
                .into_iter()
                .map(codex_app_server_protocol::UserInput::into_core)
                .collect(),
        })
        .as_legacy_events(false),
        ThreadItem::AgentMessage {
            id, text, phase, ..
        } => TurnItem::AgentMessage(AgentMessageItem {
            id: id.clone(),
            content: vec![AgentMessageContent::Text { text: text.clone() }],
            phase: phase.clone(),
        })
        .as_legacy_events(false),
        ThreadItem::Reasoning {
            id,
            summary,
            content,
        } => TurnItem::Reasoning(ReasoningItem {
            id: id.clone(),
            summary_text: summary.clone(),
            raw_content: content.clone(),
        })
        .as_legacy_events(false),
        ThreadItem::WebSearch {
            id,
            query,
            action: Some(action),
        } => TurnItem::WebSearch(WebSearchItem {
            id: id.clone(),
            query: query.clone(),
            action: match action {
                codex_app_server_protocol::WebSearchAction::Search { query, queries } => {
                    codex_protocol::models::WebSearchAction::Search {
                        query: query.clone(),
                        queries: queries.clone(),
                    }
                }
                codex_app_server_protocol::WebSearchAction::OpenPage { url } => {
                    codex_protocol::models::WebSearchAction::OpenPage { url: url.clone() }
                }
                codex_app_server_protocol::WebSearchAction::FindInPage { url, pattern } => {
                    codex_protocol::models::WebSearchAction::FindInPage {
                        url: url.clone(),
                        pattern: pattern.clone(),
                    }
                }
                codex_app_server_protocol::WebSearchAction::Other => {
                    codex_protocol::models::WebSearchAction::Other
                }
            },
        })
        .as_legacy_events(false),
        ThreadItem::ContextCompaction { id } => {
            TurnItem::ContextCompaction(ContextCompactionItem { id: id.clone() })
                .as_legacy_events(false)
        }
        ThreadItem::ImageGeneration {
            id,
            status,
            revised_prompt,
            result,
        } => TurnItem::ImageGeneration(codex_protocol::items::ImageGenerationItem {
            id: id.clone(),
            status: status.clone(),
            revised_prompt: revised_prompt.clone(),
            result: result.clone(),
        })
        .as_legacy_events(false),
        ThreadItem::Plan { .. }
        | ThreadItem::CommandExecution { .. }
        | ThreadItem::FileChange { .. }
        | ThreadItem::McpToolCall { .. }
        | ThreadItem::DynamicToolCall { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::WebSearch { action: None, .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. } => Vec::new(),
    }
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
                                sandbox_policy: Some(sandbox_policy.into()),
                                model: Some(model),
                                effort,
                                summary: None,
                                personality,
                                output_schema: final_output_json_schema,
                                collaboration_mode: None,
                            })
                            .unwrap_or_default(),
                        ),
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
        network: value.network,
        file_system: value.file_system.map(|permissions| {
            codex_protocol::models::FileSystemPermissions {
                read: permissions.read,
                write: permissions.write,
            }
        }),
        macos: value
            .macos
            .map(|permissions| codex_protocol::models::MacOsPermissions {
                preferences: permissions.preferences,
                automations: permissions.automations,
                accessibility: permissions.accessibility,
                calendar: permissions.calendar,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use codex_app_server_protocol::AskForApproval as RemoteAskForApproval;
    use codex_app_server_protocol::CommandExecutionStatus;
    use codex_app_server_protocol::SandboxPolicy as RemoteSandboxPolicy;
    use codex_app_server_protocol::SessionSource as RemoteSessionSource;
    use codex_app_server_protocol::ThreadStatus;
    use codex_protocol::config_types::ReasoningSummary;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::ReadOnlyAccess;
    use codex_protocol::protocol::SandboxPolicy;
    use codex_protocol::user_input::UserInput;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::error::TryRecvError;
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn user_turn_op_sends_turn_start_request() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/tmp/original"),
            3,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);
        let (outbound_tx, mut outbound_rx) = unbounded_channel();
        let (codex_op_tx, codex_op_rx) = unbounded_channel();

        let task = tokio::spawn(op_task(
            codex_op_rx,
            outbound_tx,
            state.clone(),
            app_event_tx,
        ));

        codex_op_tx
            .send(Op::UserTurn {
                items: vec![UserInput::Text {
                    text: "hello from connected mode".to_string(),
                    text_elements: Vec::new(),
                }],
                cwd: PathBuf::from("/repo"),
                approval_policy: AskForApproval::Never,
                sandbox_policy: SandboxPolicy::ReadOnly {
                    access: ReadOnlyAccess::FullAccess,
                },
                model: "mock-model".to_string(),
                effort: None,
                summary: ReasoningSummary::Auto,
                final_output_json_schema: None,
                collaboration_mode: None,
                personality: None,
            })
            .expect("send user turn op");
        drop(codex_op_tx);

        let outbound = timeout(Duration::from_secs(1), outbound_rx.recv())
            .await
            .expect("turn/start request should arrive")
            .expect("turn/start request should be present");
        task.await.expect("op task should finish cleanly");

        match outbound {
            JSONRPCMessage::Request(request) => {
                assert_eq!(request.id, RequestId::Integer(3));
                assert_eq!(request.method, "turn/start");

                let params: TurnStartParams =
                    serde_json::from_value(request.params.expect("turn/start params"))
                        .expect("decode turn/start params");
                assert_eq!(params.thread_id, "thread-1");
                assert_eq!(params.cwd, Some(PathBuf::from("/repo")));
                assert_eq!(params.approval_policy, Some(RemoteAskForApproval::Never));
                assert_eq!(
                    params.sandbox_policy,
                    Some(RemoteSandboxPolicy::ReadOnly {
                        access: codex_app_server_protocol::ReadOnlyAccess::FullAccess,
                    })
                );
                assert_eq!(params.model, Some("mock-model".to_string()));
                assert_eq!(
                    params.input,
                    vec![codex_app_server_protocol::UserInput::Text {
                        text: "hello from connected mode".to_string(),
                        text_elements: Vec::new(),
                    }]
                );
            }
            other => panic!("expected turn/start request, got {other:?}"),
        }

        let guard = state.lock().await;
        assert_eq!(guard.thread_cwd, PathBuf::from("/repo"));
        assert_eq!(guard.next_request_id, 4);
        drop(guard);

        assert!(matches!(
            app_event_rx.try_recv(),
            Err(TryRecvError::Empty | TryRecvError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn exec_approval_round_trip_ignores_legacy_duplicate() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/repo"),
            9,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);
        let (outbound_tx, mut outbound_rx) = unbounded_channel();

        handle_server_request(
            JSONRPCRequest {
                id: RequestId::Integer(41),
                method: "item/commandExecution/requestApproval".to_string(),
                params: Some(
                    serde_json::to_value(CommandExecutionRequestApprovalParams {
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        item_id: "call-1".to_string(),
                        approval_id: None,
                        reason: Some("needs approval".to_string()),
                        network_approval_context: None,
                        command: Some("echo hi".to_string()),
                        cwd: Some(PathBuf::from("/repo")),
                        command_actions: Some(Vec::new()),
                        additional_permissions: None,
                        proposed_execpolicy_amendment: None,
                        proposed_network_policy_amendments: None,
                        available_decisions: None,
                    })
                    .expect("serialize exec approval params"),
                ),
            },
            &outbound_tx,
            &state,
            &app_event_tx,
        )
        .await;

        let event = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("approval event should arrive")
            .expect("approval event should be present");
        match event {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::ExecApprovalRequest(request),
                ..
            }) => {
                assert_eq!(request.call_id, "call-1");
                assert_eq!(request.turn_id, "turn-1");
                assert_eq!(request.command, vec!["echo hi".to_string()]);
                assert_eq!(request.cwd, PathBuf::from("/repo"));
                assert_eq!(request.reason, Some("needs approval".to_string()));
            }
            other => panic!("expected exec approval event, got {other:?}"),
        }

        handle_notification(
            JSONRPCNotification {
                method: "codex/event/exec_approval_request".to_string(),
                params: Some(
                    serde_json::to_value(Event {
                        id: "dup".to_string(),
                        msg: EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
                            call_id: "call-1".to_string(),
                            approval_id: None,
                            turn_id: "turn-1".to_string(),
                            command: vec!["echo hi".to_string()],
                            cwd: PathBuf::from("/repo"),
                            reason: Some("needs approval".to_string()),
                            network_approval_context: None,
                            proposed_execpolicy_amendment: None,
                            proposed_network_policy_amendments: None,
                            additional_permissions: None,
                            available_decisions: None,
                            parsed_cmd: Vec::new(),
                        }),
                    })
                    .expect("serialize legacy approval event"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        assert!(matches!(app_event_rx.try_recv(), Err(TryRecvError::Empty)));

        let (codex_op_tx, codex_op_rx) = unbounded_channel();
        let task = tokio::spawn(op_task(
            codex_op_rx,
            outbound_tx,
            state.clone(),
            app_event_tx,
        ));

        codex_op_tx
            .send(Op::ExecApproval {
                id: "call-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                decision: ReviewDecision::Approved,
            })
            .expect("send exec approval op");
        drop(codex_op_tx);

        let outbound = timeout(Duration::from_secs(1), outbound_rx.recv())
            .await
            .expect("approval response should arrive")
            .expect("approval response should be present");
        task.await.expect("op task should finish cleanly");

        match outbound {
            JSONRPCMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Integer(41));
                let body: CommandExecutionRequestApprovalResponse =
                    serde_json::from_value(response.result).expect("decode exec approval response");
                assert_eq!(body.decision, CommandExecutionApprovalDecision::Accept);
            }
            other => panic!("expected exec approval response, got {other:?}"),
        }

        let guard = state.lock().await;
        assert!(guard.pending_exec_approvals.is_empty());
    }

    #[test]
    fn resumed_thread_synthesizes_initial_messages() {
        let event = session_configured_event_from_thread_response(
            Thread {
                id: ThreadId::new().to_string(),
                preview: "hello".to_string(),
                model_provider: "openai".to_string(),
                created_at: 1,
                updated_at: 2,
                status: ThreadStatus::Idle,
                path: Some(PathBuf::from("/tmp/thread.jsonl")),
                cwd: PathBuf::from("/repo"),
                cli_version: "0.106.0".to_string(),
                source: RemoteSessionSource::Cli,
                agent_nickname: None,
                agent_role: None,
                git_info: None,
                name: Some("saved thread".to_string()),
                turns: vec![
                    Turn {
                        id: "turn-1".to_string(),
                        items: vec![
                            ThreadItem::UserMessage {
                                id: "user-1".to_string(),
                                content: vec![codex_app_server_protocol::UserInput::Text {
                                    text: "hello".to_string(),
                                    text_elements: Vec::new(),
                                }],
                            },
                            ThreadItem::AgentMessage {
                                id: "agent-1".to_string(),
                                text: "world".to_string(),
                                phase: None,
                            },
                            ThreadItem::Reasoning {
                                id: "reason-1".to_string(),
                                summary: vec!["thinking".to_string()],
                                content: vec!["raw".to_string()],
                            },
                            ThreadItem::WebSearch {
                                id: "search-1".to_string(),
                                query: "codex".to_string(),
                                action: Some(codex_app_server_protocol::WebSearchAction::Search {
                                    query: Some("codex".to_string()),
                                    queries: None,
                                }),
                            },
                            ThreadItem::ContextCompaction {
                                id: "compact-1".to_string(),
                            },
                            ThreadItem::CommandExecution {
                                id: "cmd-1".to_string(),
                                command: "echo hi".to_string(),
                                cwd: PathBuf::from("/repo"),
                                process_id: None,
                                status: CommandExecutionStatus::Completed,
                                command_actions: Vec::new(),
                                aggregated_output: Some("hi".to_string()),
                                exit_code: Some(0),
                                duration_ms: Some(1),
                            },
                        ],
                        status: TurnStatus::Completed,
                        error: None,
                    },
                    Turn {
                        id: "turn-2".to_string(),
                        items: vec![ThreadItem::AgentMessage {
                            id: "agent-2".to_string(),
                            text: "in progress".to_string(),
                            phase: None,
                        }],
                        status: TurnStatus::InProgress,
                        error: None,
                    },
                ],
            },
            "mock-model".to_string(),
            "openai".to_string(),
            PathBuf::from("/repo"),
            AskForApproval::Never,
            SandboxPolicy::DangerFullAccess,
            None,
        )
        .expect("session configured event");

        match event.msg {
            EventMsg::SessionConfigured(session) => {
                assert_eq!(session.thread_name, Some("saved thread".to_string()));
                assert_eq!(session.cwd, PathBuf::from("/repo"));
                assert_eq!(
                    session.rollout_path,
                    Some(PathBuf::from("/tmp/thread.jsonl"))
                );

                let initial_messages = session
                    .initial_messages
                    .expect("resume should synthesize initial messages");
                assert_eq!(initial_messages.len(), 5);

                match &initial_messages[0] {
                    EventMsg::UserMessage(message) => {
                        assert_eq!(message.message, "hello");
                    }
                    other => panic!("expected user message, got {other:?}"),
                }
                match &initial_messages[1] {
                    EventMsg::AgentMessage(message) => {
                        assert_eq!(message.message, "world");
                    }
                    other => panic!("expected agent message, got {other:?}"),
                }
                match &initial_messages[2] {
                    EventMsg::AgentReasoning(reasoning) => {
                        assert_eq!(reasoning.text, "thinking");
                    }
                    other => panic!("expected reasoning event, got {other:?}"),
                }
                match &initial_messages[3] {
                    EventMsg::WebSearchEnd(search) => {
                        assert_eq!(search.call_id, "search-1");
                        assert_eq!(search.query, "codex");
                    }
                    other => panic!("expected web search event, got {other:?}"),
                }
                match &initial_messages[4] {
                    EventMsg::ContextCompacted(_) => {}
                    other => panic!("expected context compaction event, got {other:?}"),
                }
            }
            other => panic!("expected session configured event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn disconnect_requests_fatal_exit() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/repo"),
            3,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);
        let (outbound_tx, _outbound_rx) = unbounded_channel();

        reader_task(
            futures::stream::empty::<
                std::result::Result<WebSocketMessage, tokio_tungstenite::tungstenite::Error>,
            >(),
            outbound_tx,
            state,
            app_event_tx,
        )
        .await;

        let event = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("disconnect event should arrive")
            .expect("disconnect event should be present");
        match event {
            AppEvent::FatalExitRequest(message) => {
                assert_eq!(message, "app-server websocket connection closed");
            }
            other => panic!("expected fatal exit request, got {other:?}"),
        }
    }
}
