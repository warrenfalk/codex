use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::CommandExecutionRequestApprovalResponse;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::FileChangeRequestApprovalParams;
use codex_app_server_protocol::FileChangeRequestApprovalResponse;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::HookCompletedNotification;
use codex_app_server_protocol::HookStartedNotification;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadSortKey as RemoteThreadSortKey;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_core::ThreadSortKey;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::approvals::ExecApprovalRequestEvent;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem;
use codex_protocol::dynamic_tools::DynamicToolCallRequest;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::ContextCompactionItem;
use codex_protocol::items::ImageGenerationItem;
use codex_protocol::items::PlanItem;
use codex_protocol::items::ReasoningItem;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::items::WebSearchItem;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use codex_protocol::protocol::AgentMessageContentDeltaEvent;
use codex_protocol::protocol::DeprecationNoticeEvent;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ExecCommandBeginEvent;
use codex_protocol::protocol::ExecCommandEndEvent;
use codex_protocol::protocol::ExecCommandOutputDeltaEvent;
use codex_protocol::protocol::ExecCommandSource;
use codex_protocol::protocol::ExecCommandStatus;
use codex_protocol::protocol::ExecOutputStream;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::HasLegacyEvent;
use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookStartedEvent;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;
use codex_protocol::protocol::McpInvocation;
use codex_protocol::protocol::McpToolCallBeginEvent;
use codex_protocol::protocol::McpToolCallEndEvent;
use codex_protocol::protocol::ModelRerouteEvent;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::PatchApplyBeginEvent;
use codex_protocol::protocol::PatchApplyEndEvent;
use codex_protocol::protocol::ReasoningContentDeltaEvent;
use codex_protocol::protocol::ReasoningRawContentDeltaEvent;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_protocol::protocol::StreamErrorEvent;
use codex_protocol::protocol::TerminalInteractionEvent;
use codex_protocol::protocol::ThreadNameUpdatedEvent;
use codex_protocol::protocol::TokenCountEvent;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TokenUsageInfo;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnDiffEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::ViewImageToolCallEvent;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::request_user_input::RequestUserInputEvent;
use codex_protocol::request_user_input::RequestUserInputQuestion as CoreRequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption as CoreRequestUserInputQuestionOption;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use futures::SinkExt;
use futures::StreamExt;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;
use tokio_util::sync::CancellationToken;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

#[derive(Clone, Debug)]
struct PendingRequest {
    kind: &'static str,
}

#[derive(Debug)]
struct ConnectedSessionState {
    current_turn_id: Option<String>,
    current_turn_last_agent_message: Option<String>,
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
            current_turn_last_agent_message: None,
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
    pub(crate) handle: ConnectedSessionHandle,
}

pub(crate) enum ConnectedSessionMode {
    StartFresh,
    Resume { thread_id: String },
    Fork { thread_id: String },
}

pub(crate) struct ConnectedSessionHandle {
    cancel: CancellationToken,
    outbound_tx: UnboundedSender<JSONRPCMessage>,
    state: std::sync::Arc<Mutex<ConnectedSessionState>>,
    tasks: Vec<JoinHandle<()>>,
}

impl ConnectedSessionHandle {
    pub(crate) async fn shutdown(self) {
        let request_id = {
            let mut guard = self.state.lock().await;
            let request_id = guard.allocate_request_id("thread/unsubscribe");
            let request = JSONRPCMessage::Request(JSONRPCRequest {
                id: request_id.clone(),
                method: "thread/unsubscribe".to_string(),
                params: Some(
                    serde_json::to_value(ThreadUnsubscribeParams {
                        thread_id: guard.thread_id.clone(),
                    })
                    .unwrap_or_default(),
                ),
                trace: None,
            });
            let _ = self.outbound_tx.send(request);
            request_id
        };

        let _ = tokio::time::timeout(std::time::Duration::from_millis(250), async {
            loop {
                {
                    let guard = self.state.lock().await;
                    if !guard.pending_requests.contains_key(&request_id) {
                        break;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await;

        self.cancel.cancel();
        for task in self.tasks {
            task.abort();
            let _ = task.await;
        }
    }
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

    let cancel = CancellationToken::new();
    let writer_cancel = cancel.clone();
    let reader_cancel = cancel.clone();
    let op_cancel = cancel.clone();
    let writer = tokio::spawn(writer_task(
        write,
        outbound_rx,
        writer_cancel,
        app_event_tx.clone(),
    ));
    let reader = tokio::spawn(reader_task(
        read,
        outbound_tx.clone(),
        state.clone(),
        reader_cancel,
        app_event_tx.clone(),
    ));
    let op = tokio::spawn(op_task(
        codex_op_rx,
        outbound_tx.clone(),
        state.clone(),
        op_cancel,
        app_event_tx.clone(),
    ));

    Ok(ConnectedSessionBootstrap {
        codex_op_tx,
        session_configured_event,
        handle: ConnectedSessionHandle {
            cancel,
            outbound_tx,
            state,
            tasks: vec![writer, reader, op],
        },
    })
}

pub(crate) async fn find_latest_thread_id(url: &str, cwd: Option<&Path>) -> Result<Option<String>> {
    Ok(
        list_threads_page(url, None, 1, ThreadSortKey::UpdatedAt, None, cwd, None)
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
    search_term: Option<&str>,
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
            search_term: search_term.map(ToOwned::to_owned),
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

async fn initialize_connection<S>(ws: &mut WebSocketStream<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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

async fn open_thread_for_session<S>(
    ws: &mut WebSocketStream<S>,
    config: &Config,
    mode: ConnectedSessionMode,
) -> Result<(ThreadId, PathBuf, Event)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
                    service_tier: config.service_tier.map(Some),
                    cwd: Some(config.cwd.display().to_string()),
                    approval_policy: Some(config.permissions.approval_policy.value().into()),
                    approvals_reviewer: Some(config.approvals_reviewer.into()),
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
                thread_start.service_tier,
                thread_start.cwd,
                thread_start.approval_policy.to_core(),
                thread_start.approvals_reviewer.to_core(),
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
                    service_tier: config.service_tier.map(Some),
                    cwd: Some(config.cwd.display().to_string()),
                    approval_policy: Some(config.permissions.approval_policy.value().into()),
                    approvals_reviewer: Some(config.approvals_reviewer.into()),
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
                thread_resume.service_tier,
                thread_resume.cwd,
                thread_resume.approval_policy.to_core(),
                thread_resume.approvals_reviewer.to_core(),
                thread_resume.sandbox.to_core(),
                thread_resume.reasoning_effort,
            )
            .wrap_err("build session_configured event from thread/resume response")?;
            Ok((thread_id, thread_cwd, session_configured_event))
        }
        ConnectedSessionMode::Fork { thread_id } => {
            let request_id = RequestId::Integer(2);
            send_request(
                ws,
                "thread/fork",
                request_id.clone(),
                Some(serde_json::to_value(ThreadForkParams {
                    thread_id,
                    path: None,
                    model: config.model.clone(),
                    model_provider: None,
                    service_tier: config.service_tier.map(Some),
                    cwd: Some(config.cwd.display().to_string()),
                    approval_policy: Some(config.permissions.approval_policy.value().into()),
                    approvals_reviewer: Some(config.approvals_reviewer.into()),
                    sandbox: None,
                    config: None,
                    base_instructions: None,
                    developer_instructions: config.developer_instructions.clone(),
                    ephemeral: false,
                    persist_extended_history: false,
                })?),
            )
            .await?;
            let response = read_response_for_id(ws, &request_id).await?;
            let thread_fork: ThreadForkResponse =
                serde_json::from_value(response.result).wrap_err("decode thread/fork response")?;
            let thread_id = ThreadId::try_from(thread_fork.thread.id.as_str())
                .wrap_err("invalid thread id in thread/fork response")?;
            let thread_cwd = thread_fork.cwd.clone();
            let session_configured_event = session_configured_event_from_thread_response(
                thread_fork.thread,
                thread_fork.model,
                thread_fork.model_provider,
                thread_fork.service_tier,
                thread_fork.cwd,
                thread_fork.approval_policy.to_core(),
                thread_fork.approvals_reviewer.to_core(),
                thread_fork.sandbox.to_core(),
                thread_fork.reasoning_effort,
            )
            .wrap_err("build session_configured event from thread/fork response")?;
            Ok((thread_id, thread_cwd, session_configured_event))
        }
    }
}

fn session_configured_event_from_thread_response(
    thread: Thread,
    model: String,
    model_provider_id: String,
    service_tier: Option<codex_protocol::config_types::ServiceTier>,
    cwd: PathBuf,
    approval_policy: codex_protocol::protocol::AskForApproval,
    approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer,
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
            service_tier,
            approval_policy,
            approvals_reviewer,
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
            memory_citation: None,
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
            saved_path,
        } => TurnItem::ImageGeneration(codex_protocol::items::ImageGenerationItem {
            id: id.clone(),
            status: status.clone(),
            revised_prompt: revised_prompt.clone(),
            result: result.clone(),
            saved_path: saved_path.clone(),
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
    cancel: CancellationToken,
    app_event_tx: AppEventSender,
) where
    S: futures::Sink<WebSocketMessage, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin
        + Send
        + 'static,
{
    loop {
        let message = tokio::select! {
            _ = cancel.cancelled() => break,
            message = outbound_rx.recv() => match message {
                Some(message) => message,
                None => break,
            },
        };
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
            if !cancel.is_cancelled() {
                app_event_tx.send(AppEvent::FatalExitRequest(format!(
                    "app-server websocket send failed: {err}"
                )));
            }
            break;
        }
    }
}

async fn reader_task<S>(
    mut read: S,
    outbound_tx: UnboundedSender<JSONRPCMessage>,
    state: std::sync::Arc<Mutex<ConnectedSessionState>>,
    cancel: CancellationToken,
    app_event_tx: AppEventSender,
) where
    S: futures::Stream<
            Item = std::result::Result<WebSocketMessage, tokio_tungstenite::tungstenite::Error>,
        > + Unpin
        + Send
        + 'static,
{
    loop {
        let frame = match tokio::select! {
            _ = cancel.cancelled() => break,
            frame = read.next() => frame,
        } {
            Some(Ok(frame)) => frame,
            Some(Err(err)) => {
                if !cancel.is_cancelled() {
                    app_event_tx.send(AppEvent::FatalExitRequest(format!(
                        "app-server websocket read failed: {err}"
                    )));
                }
                break;
            }
            None => {
                if !cancel.is_cancelled() {
                    app_event_tx.send(AppEvent::FatalExitRequest(
                        "app-server websocket connection closed".to_string(),
                    ));
                }
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
    cancel: CancellationToken,
    app_event_tx: AppEventSender,
) {
    loop {
        let op = tokio::select! {
            _ = cancel.cancelled() => break,
            op = codex_op_rx.recv() => match op {
                Some(op) => op,
                None => break,
            },
        };
        let op_name = op.kind();
        match op {
            Op::UserTurn {
                items,
                cwd,
                approval_policy,
                sandbox_policy,
                model,
                service_tier,
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
                                service_tier,
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
            Op::SetThreadName { name } => {
                let request = {
                    let mut guard = state.lock().await;
                    let request_id = guard.allocate_request_id("thread/name/set");
                    JSONRPCMessage::Request(JSONRPCRequest {
                        id: request_id,
                        method: "thread/name/set".to_string(),
                        params: Some(
                            serde_json::to_value(ThreadSetNameParams {
                                thread_id: guard.thread_id.clone(),
                                name,
                            })
                            .unwrap_or_default(),
                        ),
                        trace: None,
                    })
                };
                if outbound_tx.send(request).is_err() {
                    send_error_event(
                        &app_event_tx,
                        "failed to send thread/name/set to app-server".to_string(),
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
                    format!("{op_name} is currently ignored in connected mode"),
                );
            }
        }
    }
}

async fn send_request<S>(
    ws: &mut WebSocketStream<S>,
    method: &str,
    id: RequestId,
    params: Option<serde_json::Value>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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

async fn send_notification<S>(
    ws: &mut WebSocketStream<S>,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let message = JSONRPCMessage::Notification(JSONRPCNotification {
        method: method.to_string(),
        params,
    });
    let payload = serde_json::to_string(&message)?;
    ws.send(WebSocketMessage::Text(payload.into()))
        .await
        .wrap_err("failed to send websocket frame")
}

async fn read_response_for_id<S>(
    ws: &mut WebSocketStream<S>,
    request_id: &RequestId,
) -> Result<JSONRPCResponse>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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

async fn read_jsonrpc_message<S>(ws: &mut WebSocketStream<S>) -> Result<JSONRPCMessage>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
    if notification.method.starts_with("codex/event/") {
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

        forward_connected_events(vec![event], state, app_event_tx).await;
        return;
    }

    let server_notification = match ServerNotification::try_from(notification.clone()) {
        Ok(server_notification) => server_notification,
        Err(_) => return,
    };
    if let ServerNotification::ThreadActivationRequested(notification) = &server_notification {
        handle_thread_activation_requested(notification, state, app_event_tx).await;
        return;
    }
    let Ok(events) = events_from_server_notification(server_notification) else {
        send_error_event(
            app_event_tx,
            format!(
                "failed to translate app-server notification `{}` for connected mode",
                notification.method
            ),
        );
        return;
    };

    forward_connected_events(events, state, app_event_tx).await;
}

async fn handle_thread_activation_requested(
    notification: &codex_app_server_protocol::ThreadActivationRequestedNotification,
    state: &std::sync::Arc<Mutex<ConnectedSessionState>>,
    app_event_tx: &AppEventSender,
) {
    let thread_id_matches = {
        let guard = state.lock().await;
        guard.thread_id == notification.thread_id
    };
    if !thread_id_matches {
        return;
    }

    if let Err(err) = crate::kitty::focus_current_window().await {
        tracing::warn!(
            thread_id = notification.thread_id,
            error = %err,
            "failed to focus current kitty window for thread activation request"
        );
        send_warning_event(
            app_event_tx,
            format!("Failed to focus this session window: {err}"),
        );
    }
}

async fn forward_connected_events(
    events: Vec<Event>,
    state: &std::sync::Arc<Mutex<ConnectedSessionState>>,
    app_event_tx: &AppEventSender,
) {
    for mut event in events {
        let skip = {
            let mut guard = state.lock().await;
            match &mut event.msg {
                // In connected mode, app-server sends these as server requests as well as
                // legacy codex/event notifications. The request path is the only one that
                // carries a JSON-RPC callback id, so processing the legacy event here
                // duplicates the prompt and leaves the second approval unresolved.
                EventMsg::ExecApprovalRequest(_)
                | EventMsg::ApplyPatchApprovalRequest(_)
                | EventMsg::RequestUserInput(_) => true,
                EventMsg::TurnStarted(turn_started) => {
                    guard.current_turn_id = Some(turn_started.turn_id.clone());
                    guard.current_turn_last_agent_message = None;
                    false
                }
                EventMsg::TurnComplete(turn_complete) => {
                    if guard.current_turn_id.as_deref() == Some(turn_complete.turn_id.as_str()) {
                        if turn_complete.last_agent_message.is_none() {
                            turn_complete.last_agent_message =
                                guard.current_turn_last_agent_message.take();
                        } else {
                            guard.current_turn_last_agent_message = None;
                        }
                        guard.current_turn_id = None;
                    }
                    false
                }
                EventMsg::AgentMessageContentDelta(delta) => {
                    if guard.current_turn_id.as_deref() == Some(delta.turn_id.as_str()) {
                        guard
                            .current_turn_last_agent_message
                            .get_or_insert_with(String::new)
                            .push_str(&delta.delta);
                    }
                    false
                }
                EventMsg::ItemCompleted(ItemCompletedEvent {
                    turn_id,
                    item: TurnItem::AgentMessage(item),
                    ..
                }) => {
                    if guard.current_turn_id.as_deref() == Some(turn_id.as_str()) {
                        guard.current_turn_last_agent_message = Some(
                            item.content
                                .iter()
                                .map(|content| match content {
                                    AgentMessageContent::Text { text } => text.as_str(),
                                })
                                .collect(),
                        );
                    }
                    false
                }
                EventMsg::TurnAborted(turn_aborted) => {
                    if guard.current_turn_id.as_deref() == turn_aborted.turn_id.as_deref() {
                        guard.current_turn_id = None;
                        guard.current_turn_last_agent_message = None;
                    }
                    false
                }
                EventMsg::ShutdownComplete => {
                    guard.current_turn_id = None;
                    guard.current_turn_last_agent_message = None;
                    false
                }
                _ => false,
            }
        };
        if !skip {
            app_event_tx.send(AppEvent::CodexEvent(event));
        }
    }
}

fn events_from_server_notification(notification: ServerNotification) -> Result<Vec<Event>> {
    let messages = match notification {
        ServerNotification::Error(notification) => {
            let msg = if notification.will_retry {
                EventMsg::StreamError(StreamErrorEvent {
                    message: notification.error.message,
                    codex_error_info: notification
                        .error
                        .codex_error_info
                        .map(codex_error_info_to_core),
                    additional_details: notification.error.additional_details,
                })
            } else {
                EventMsg::Error(ErrorEvent {
                    message: notification.error.message,
                    codex_error_info: notification
                        .error
                        .codex_error_info
                        .map(codex_error_info_to_core),
                })
            };
            vec![msg]
        }
        ServerNotification::ThreadNameUpdated(notification) => {
            let thread_id = ThreadId::try_from(notification.thread_id.as_str())
                .wrap_err("invalid thread id in thread/name/updated notification")?;
            vec![EventMsg::ThreadNameUpdated(ThreadNameUpdatedEvent {
                thread_id,
                thread_name: notification.thread_name,
            })]
        }
        ServerNotification::ThreadActivationRequested(_) => Vec::new(),
        ServerNotification::ThreadTokenUsageUpdated(notification) => {
            vec![EventMsg::TokenCount(thread_token_usage_to_token_count(
                notification,
            ))]
        }
        ServerNotification::TurnStarted(notification) => {
            vec![EventMsg::TurnStarted(turn_started_event_from_notification(
                notification,
            ))]
        }
        ServerNotification::TurnCompleted(notification) => {
            turn_completion_messages_from_notification(notification)
        }
        ServerNotification::HookStarted(notification) => {
            vec![EventMsg::HookStarted(hook_started_event_from_notification(
                notification,
            ))]
        }
        ServerNotification::HookCompleted(notification) => {
            vec![EventMsg::HookCompleted(
                hook_completed_event_from_notification(notification),
            )]
        }
        ServerNotification::TurnDiffUpdated(notification) => {
            vec![EventMsg::TurnDiff(TurnDiffEvent {
                unified_diff: notification.diff,
            })]
        }
        ServerNotification::TurnPlanUpdated(notification) => {
            vec![EventMsg::PlanUpdate(turn_plan_update_from_notification(
                notification,
            ))]
        }
        ServerNotification::ItemStarted(notification) => item_messages_from_notification(
            notification.thread_id,
            notification.turn_id,
            notification.item,
            true,
        )?,
        ServerNotification::ItemCompleted(notification) => item_messages_from_notification(
            notification.thread_id,
            notification.turn_id,
            notification.item,
            false,
        )?,
        ServerNotification::AgentMessageDelta(notification) => event_messages_with_legacy(
            EventMsg::AgentMessageContentDelta(AgentMessageContentDeltaEvent {
                thread_id: notification.thread_id,
                turn_id: notification.turn_id,
                item_id: notification.item_id,
                delta: notification.delta,
            }),
        ),
        ServerNotification::PlanDelta(notification) => vec![EventMsg::PlanDelta(
            codex_protocol::protocol::PlanDeltaEvent {
                thread_id: notification.thread_id,
                turn_id: notification.turn_id,
                item_id: notification.item_id,
                delta: notification.delta,
            },
        )],
        ServerNotification::ReasoningSummaryTextDelta(notification) => event_messages_with_legacy(
            EventMsg::ReasoningContentDelta(ReasoningContentDeltaEvent {
                thread_id: notification.thread_id,
                turn_id: notification.turn_id,
                item_id: notification.item_id,
                delta: notification.delta,
                summary_index: notification.summary_index,
            }),
        ),
        ServerNotification::ReasoningTextDelta(notification) => event_messages_with_legacy(
            EventMsg::ReasoningRawContentDelta(ReasoningRawContentDeltaEvent {
                thread_id: notification.thread_id,
                turn_id: notification.turn_id,
                item_id: notification.item_id,
                delta: notification.delta,
                content_index: notification.content_index,
            }),
        ),
        ServerNotification::TerminalInteraction(notification) => {
            vec![EventMsg::TerminalInteraction(TerminalInteractionEvent {
                call_id: notification.item_id,
                process_id: notification.process_id,
                stdin: notification.stdin,
            })]
        }
        ServerNotification::CommandExecutionOutputDelta(notification) => {
            vec![EventMsg::ExecCommandOutputDelta(
                ExecCommandOutputDeltaEvent {
                    call_id: notification.item_id,
                    stream: ExecOutputStream::Stdout,
                    chunk: notification.delta.into_bytes(),
                },
            )]
        }
        ServerNotification::SkillsChanged(_) => vec![EventMsg::SkillsUpdateAvailable],
        ServerNotification::ModelRerouted(notification) => {
            vec![EventMsg::ModelReroute(ModelRerouteEvent {
                from_model: notification.from_model,
                to_model: notification.to_model,
                reason: notification.reason.to_core(),
            })]
        }
        ServerNotification::DeprecationNotice(notification) => {
            vec![EventMsg::DeprecationNotice(DeprecationNoticeEvent {
                summary: notification.summary,
                details: notification.details,
            })]
        }
        _ => Vec::new(),
    };

    Ok(messages
        .into_iter()
        .map(|msg| Event {
            id: String::new(),
            msg,
        })
        .collect())
}

fn turn_started_event_from_notification(notification: TurnStartedNotification) -> TurnStartedEvent {
    TurnStartedEvent {
        turn_id: notification.turn.id,
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }
}

fn turn_completion_messages_from_notification(
    notification: TurnCompletedNotification,
) -> Vec<EventMsg> {
    match notification.turn.status {
        TurnStatus::Completed => vec![EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: notification.turn.id,
            last_agent_message: None,
        })],
        TurnStatus::Interrupted => vec![EventMsg::TurnAborted(TurnAbortedEvent {
            turn_id: Some(notification.turn.id),
            reason: TurnAbortReason::Interrupted,
        })],
        TurnStatus::Failed => Vec::new(),
        TurnStatus::InProgress => Vec::new(),
    }
}

fn hook_started_event_from_notification(notification: HookStartedNotification) -> HookStartedEvent {
    HookStartedEvent {
        turn_id: notification.turn_id,
        run: hook_run_summary_to_core(notification.run),
    }
}

fn hook_completed_event_from_notification(
    notification: HookCompletedNotification,
) -> HookCompletedEvent {
    HookCompletedEvent {
        turn_id: notification.turn_id,
        run: hook_run_summary_to_core(notification.run),
    }
}

fn hook_run_summary_to_core(
    summary: codex_app_server_protocol::HookRunSummary,
) -> codex_protocol::protocol::HookRunSummary {
    codex_protocol::protocol::HookRunSummary {
        id: summary.id,
        event_name: summary.event_name.to_core(),
        handler_type: summary.handler_type.to_core(),
        execution_mode: summary.execution_mode.to_core(),
        scope: summary.scope.to_core(),
        source_path: summary.source_path,
        display_order: summary.display_order,
        status: summary.status.to_core(),
        status_message: summary.status_message,
        started_at: summary.started_at,
        completed_at: summary.completed_at,
        duration_ms: summary.duration_ms,
        entries: summary
            .entries
            .into_iter()
            .map(|entry| codex_protocol::protocol::HookOutputEntry {
                kind: entry.kind.to_core(),
                text: entry.text,
            })
            .collect(),
    }
}

fn turn_plan_update_from_notification(notification: TurnPlanUpdatedNotification) -> UpdatePlanArgs {
    UpdatePlanArgs {
        explanation: notification.explanation,
        plan: notification
            .plan
            .into_iter()
            .map(|step| PlanItemArg {
                step: step.step,
                status: match step.status {
                    TurnPlanStepStatus::Pending => StepStatus::Pending,
                    TurnPlanStepStatus::InProgress => StepStatus::InProgress,
                    TurnPlanStepStatus::Completed => StepStatus::Completed,
                },
            })
            .collect(),
    }
}

fn item_messages_from_notification(
    thread_id: String,
    turn_id: String,
    item: ThreadItem,
    started: bool,
) -> Result<Vec<EventMsg>> {
    if let Some(messages) = tool_item_messages(thread_id.as_str(), turn_id.as_str(), &item, started)
    {
        return Ok(messages);
    }

    let thread_id = ThreadId::try_from(thread_id.as_str())
        .wrap_err("invalid thread id in item notification")?;
    let Some(item) = core_turn_item_from_thread_item(item) else {
        return Ok(Vec::new());
    };
    let event = if started {
        EventMsg::ItemStarted(ItemStartedEvent {
            thread_id,
            turn_id,
            item,
        })
    } else {
        EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id,
            item,
        })
    };
    Ok(event_messages_with_legacy(event))
}

fn tool_item_messages(
    _thread_id: &str,
    turn_id: &str,
    item: &ThreadItem,
    started: bool,
) -> Option<Vec<EventMsg>> {
    match item {
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            process_id,
            status,
            aggregated_output,
            exit_code,
            duration_ms,
            ..
        } => {
            let command_parts = shlex::split(command).unwrap_or_else(|| vec![command.clone()]);
            Some(
                if started || matches!(status, CommandExecutionStatus::InProgress) {
                    vec![EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
                        call_id: id.clone(),
                        process_id: process_id.clone(),
                        turn_id: turn_id.to_string(),
                        command: command_parts,
                        cwd: cwd.clone(),
                        parsed_cmd: Vec::new(),
                        source: ExecCommandSource::Agent,
                        interaction_input: None,
                    })]
                } else {
                    vec![EventMsg::ExecCommandEnd(ExecCommandEndEvent {
                        call_id: id.clone(),
                        process_id: process_id.clone(),
                        turn_id: turn_id.to_string(),
                        command: command_parts,
                        cwd: cwd.clone(),
                        parsed_cmd: Vec::new(),
                        source: ExecCommandSource::Agent,
                        interaction_input: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        aggregated_output: aggregated_output.clone().unwrap_or_default(),
                        exit_code: exit_code.unwrap_or_default(),
                        duration: std::time::Duration::from_millis(
                            u64::try_from(duration_ms.unwrap_or_default()).unwrap_or_default(),
                        ),
                        formatted_output: aggregated_output.clone().unwrap_or_default(),
                        status: match status {
                            CommandExecutionStatus::InProgress
                            | CommandExecutionStatus::Completed => ExecCommandStatus::Completed,
                            CommandExecutionStatus::Failed => ExecCommandStatus::Failed,
                            CommandExecutionStatus::Declined => ExecCommandStatus::Declined,
                        },
                    })]
                },
            )
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => Some(
            if started || matches!(status, PatchApplyStatus::InProgress) {
                vec![EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
                    call_id: id.clone(),
                    turn_id: turn_id.to_string(),
                    auto_approved: true,
                    changes: file_changes_to_core(changes),
                })]
            } else {
                let success = matches!(status, PatchApplyStatus::Completed);
                vec![EventMsg::PatchApplyEnd(PatchApplyEndEvent {
                    call_id: id.clone(),
                    turn_id: turn_id.to_string(),
                    stdout: String::new(),
                    stderr: String::new(),
                    success,
                    changes: file_changes_to_core(changes),
                    status: match status {
                        PatchApplyStatus::InProgress | PatchApplyStatus::Completed => {
                            codex_protocol::protocol::PatchApplyStatus::Completed
                        }
                        PatchApplyStatus::Failed => {
                            codex_protocol::protocol::PatchApplyStatus::Failed
                        }
                        PatchApplyStatus::Declined => {
                            codex_protocol::protocol::PatchApplyStatus::Declined
                        }
                    },
                })]
            },
        ),
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            arguments,
            result,
            error,
            duration_ms,
            ..
        } => Some(if started {
            vec![EventMsg::McpToolCallBegin(McpToolCallBeginEvent {
                call_id: id.clone(),
                invocation: McpInvocation {
                    server: server.clone(),
                    tool: tool.clone(),
                    arguments: Some(arguments.clone()),
                },
            })]
        } else {
            let result = match (result.clone(), error.clone()) {
                (Some(result), _) => Ok(CallToolResult {
                    content: result.content,
                    structured_content: result.structured_content,
                    is_error: Some(false),
                    meta: None,
                }),
                (None, Some(error)) => Err(error.message),
                (None, None) => Err("MCP tool call completed without result".to_string()),
            };
            vec![EventMsg::McpToolCallEnd(McpToolCallEndEvent {
                call_id: id.clone(),
                invocation: McpInvocation {
                    server: server.clone(),
                    tool: tool.clone(),
                    arguments: Some(arguments.clone()),
                },
                duration: std::time::Duration::from_millis(
                    u64::try_from(duration_ms.unwrap_or_default()).unwrap_or_default(),
                ),
                result,
            })]
        }),
        ThreadItem::DynamicToolCall {
            id,
            tool,
            arguments,
            content_items,
            success,
            duration_ms,
            ..
        } => Some(if started {
            vec![EventMsg::DynamicToolCallRequest(DynamicToolCallRequest {
                call_id: id.clone(),
                turn_id: turn_id.to_string(),
                tool: tool.clone(),
                arguments: arguments.clone(),
            })]
        } else {
            vec![EventMsg::DynamicToolCallResponse(
                codex_protocol::protocol::DynamicToolCallResponseEvent {
                    call_id: id.clone(),
                    turn_id: turn_id.to_string(),
                    tool: tool.clone(),
                    arguments: arguments.clone(),
                    content_items: content_items
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(Into::into)
                        .collect::<Vec<DynamicToolCallOutputContentItem>>(),
                    success: success.unwrap_or(false),
                    error: None,
                    duration: std::time::Duration::from_millis(
                        u64::try_from(duration_ms.unwrap_or_default()).unwrap_or_default(),
                    ),
                },
            )]
        }),
        ThreadItem::ImageView { id, path } if !started => {
            Some(vec![EventMsg::ViewImageToolCall(ViewImageToolCallEvent {
                call_id: id.clone(),
                path: PathBuf::from(path),
            })])
        }
        ThreadItem::ContextCompaction { .. } if started => Some(Vec::new()),
        ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::ContextCompaction { .. } => Some(Vec::new()),
        ThreadItem::UserMessage { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::WebSearch { .. }
        | ThreadItem::ImageGeneration { .. } => None,
    }
}

fn core_turn_item_from_thread_item(item: ThreadItem) -> Option<TurnItem> {
    match item {
        ThreadItem::UserMessage { id, content } => Some(TurnItem::UserMessage(UserMessageItem {
            id,
            content: content
                .into_iter()
                .map(codex_app_server_protocol::UserInput::into_core)
                .collect(),
        })),
        ThreadItem::AgentMessage {
            id, text, phase, ..
        } => Some(TurnItem::AgentMessage(AgentMessageItem {
            id,
            content: vec![AgentMessageContent::Text { text }],
            phase,
            memory_citation: None,
        })),
        ThreadItem::Plan { id, text } => Some(TurnItem::Plan(PlanItem { id, text })),
        ThreadItem::Reasoning {
            id,
            summary,
            content,
        } => Some(TurnItem::Reasoning(ReasoningItem {
            id,
            summary_text: summary,
            raw_content: content,
        })),
        ThreadItem::WebSearch {
            id,
            query,
            action: Some(action),
        } => Some(TurnItem::WebSearch(WebSearchItem {
            id,
            query,
            action: match action {
                codex_app_server_protocol::WebSearchAction::Search { query, queries } => {
                    codex_protocol::models::WebSearchAction::Search { query, queries }
                }
                codex_app_server_protocol::WebSearchAction::OpenPage { url } => {
                    codex_protocol::models::WebSearchAction::OpenPage { url }
                }
                codex_app_server_protocol::WebSearchAction::FindInPage { url, pattern } => {
                    codex_protocol::models::WebSearchAction::FindInPage { url, pattern }
                }
                codex_app_server_protocol::WebSearchAction::Other => {
                    codex_protocol::models::WebSearchAction::Other
                }
            },
        })),
        ThreadItem::ImageGeneration {
            id,
            status,
            revised_prompt,
            result,
            saved_path,
        } => Some(TurnItem::ImageGeneration(ImageGenerationItem {
            id,
            status,
            revised_prompt,
            result,
            saved_path,
        })),
        ThreadItem::ContextCompaction { id } => {
            Some(TurnItem::ContextCompaction(ContextCompactionItem { id }))
        }
        ThreadItem::CommandExecution { .. }
        | ThreadItem::FileChange { .. }
        | ThreadItem::McpToolCall { .. }
        | ThreadItem::DynamicToolCall { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::WebSearch { action: None, .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. } => None,
    }
}

fn file_changes_to_core(changes: &[FileUpdateChange]) -> HashMap<PathBuf, FileChange> {
    changes
        .iter()
        .map(|change| {
            let file_change = match &change.kind {
                codex_app_server_protocol::PatchChangeKind::Add => FileChange::Add {
                    content: change.diff.clone(),
                },
                codex_app_server_protocol::PatchChangeKind::Delete => FileChange::Delete {
                    content: change.diff.clone(),
                },
                codex_app_server_protocol::PatchChangeKind::Update { move_path } => {
                    FileChange::Update {
                        unified_diff: change.diff.clone(),
                        move_path: move_path.clone(),
                    }
                }
            };
            (PathBuf::from(&change.path), file_change)
        })
        .collect()
}

fn thread_token_usage_to_token_count(
    notification: ThreadTokenUsageUpdatedNotification,
) -> TokenCountEvent {
    TokenCountEvent {
        info: Some(TokenUsageInfo {
            total_token_usage: token_usage_to_core(notification.token_usage.total),
            last_token_usage: token_usage_to_core(notification.token_usage.last),
            model_context_window: notification.token_usage.model_context_window,
        }),
        rate_limits: None,
    }
}

fn token_usage_to_core(value: codex_app_server_protocol::TokenUsageBreakdown) -> TokenUsage {
    TokenUsage {
        input_tokens: value.input_tokens,
        cached_input_tokens: value.cached_input_tokens,
        output_tokens: value.output_tokens,
        reasoning_output_tokens: value.reasoning_output_tokens,
        total_tokens: value.total_tokens,
    }
}

fn codex_error_info_to_core(
    value: codex_app_server_protocol::CodexErrorInfo,
) -> codex_protocol::protocol::CodexErrorInfo {
    match value {
        codex_app_server_protocol::CodexErrorInfo::ContextWindowExceeded => {
            codex_protocol::protocol::CodexErrorInfo::ContextWindowExceeded
        }
        codex_app_server_protocol::CodexErrorInfo::UsageLimitExceeded => {
            codex_protocol::protocol::CodexErrorInfo::UsageLimitExceeded
        }
        codex_app_server_protocol::CodexErrorInfo::ServerOverloaded => {
            codex_protocol::protocol::CodexErrorInfo::ServerOverloaded
        }
        codex_app_server_protocol::CodexErrorInfo::HttpConnectionFailed { http_status_code } => {
            codex_protocol::protocol::CodexErrorInfo::HttpConnectionFailed { http_status_code }
        }
        codex_app_server_protocol::CodexErrorInfo::ResponseStreamConnectionFailed {
            http_status_code,
        } => codex_protocol::protocol::CodexErrorInfo::ResponseStreamConnectionFailed {
            http_status_code,
        },
        codex_app_server_protocol::CodexErrorInfo::InternalServerError => {
            codex_protocol::protocol::CodexErrorInfo::InternalServerError
        }
        codex_app_server_protocol::CodexErrorInfo::Unauthorized => {
            codex_protocol::protocol::CodexErrorInfo::Unauthorized
        }
        codex_app_server_protocol::CodexErrorInfo::BadRequest => {
            codex_protocol::protocol::CodexErrorInfo::BadRequest
        }
        codex_app_server_protocol::CodexErrorInfo::ThreadRollbackFailed => {
            codex_protocol::protocol::CodexErrorInfo::ThreadRollbackFailed
        }
        codex_app_server_protocol::CodexErrorInfo::SandboxError => {
            codex_protocol::protocol::CodexErrorInfo::SandboxError
        }
        codex_app_server_protocol::CodexErrorInfo::ResponseStreamDisconnected {
            http_status_code,
        } => codex_protocol::protocol::CodexErrorInfo::ResponseStreamDisconnected {
            http_status_code,
        },
        codex_app_server_protocol::CodexErrorInfo::ResponseTooManyFailedAttempts {
            http_status_code,
        } => codex_protocol::protocol::CodexErrorInfo::ResponseTooManyFailedAttempts {
            http_status_code,
        },
        codex_app_server_protocol::CodexErrorInfo::Other => {
            codex_protocol::protocol::CodexErrorInfo::Other
        }
    }
}

fn event_messages_with_legacy(message: EventMsg) -> Vec<EventMsg> {
    let mut messages = vec![message.clone()];
    messages.extend(message.as_legacy_events(false));
    messages
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use codex_app_server_protocol::AdditionalFileSystemPermissions;
    use codex_app_server_protocol::AdditionalMacOsPermissions;
    use codex_app_server_protocol::AdditionalNetworkPermissions;
    use codex_app_server_protocol::AdditionalPermissionProfile;
    use codex_app_server_protocol::ApprovalsReviewer as RemoteApprovalsReviewer;
    use codex_app_server_protocol::AskForApproval as RemoteAskForApproval;
    use codex_app_server_protocol::CommandExecutionStatus;
    use codex_app_server_protocol::SandboxPolicy as RemoteSandboxPolicy;
    use codex_app_server_protocol::SessionSource as RemoteSessionSource;
    use codex_app_server_protocol::ThreadStatus;
    use codex_core::config::ConfigBuilder;
    use codex_protocol::models::FileSystemPermissions;
    use codex_protocol::models::MacOsAutomationPermission;
    use codex_protocol::models::MacOsContactsPermission;
    use codex_protocol::models::MacOsPreferencesPermission;
    use codex_protocol::models::MacOsSeatbeltProfileExtensions;
    use codex_protocol::models::NetworkPermissions;
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::ReadOnlyAccess;
    use codex_protocol::protocol::SandboxPolicy;
    use codex_protocol::user_input::UserInput;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;
    use tokio::io::duplex;
    use tokio::sync::mpsc::error::TryRecvError;
    use tokio::time::timeout;
    use tokio_tungstenite::tungstenite::protocol::Role;
    use tokio_util::sync::CancellationToken;

    use super::*;

    #[test]
    fn synthesize_initial_messages_preserves_image_generation_saved_path() {
        let item = ThreadItem::ImageGeneration {
            id: "img-1".to_string(),
            status: "completed".to_string(),
            revised_prompt: Some("revised".to_string()),
            result: "base64".to_string(),
            saved_path: Some("/tmp/output.png".to_string()),
        };

        let events = synthesize_initial_messages_from_item(&item);

        assert_eq!(events.len(), 1);
        match &events[0] {
            EventMsg::ImageGenerationEnd(image) => {
                assert_eq!(image.saved_path.as_deref(), Some("/tmp/output.png"));
            }
            other => panic!("expected completed image generation event, got {other:?}"),
        }
    }

    #[test]
    fn additional_permission_profile_maps_macos_extensions_to_core() {
        let profile = AdditionalPermissionProfile {
            network: Some(AdditionalNetworkPermissions {
                enabled: Some(true),
            }),
            file_system: Some(AdditionalFileSystemPermissions {
                read: Some(vec![
                    AbsolutePathBuf::from_absolute_path("/read").expect("absolute read path"),
                ]),
                write: Some(vec![
                    AbsolutePathBuf::from_absolute_path("/write").expect("absolute write path"),
                ]),
            }),
            macos: Some(AdditionalMacOsPermissions {
                preferences: MacOsPreferencesPermission::ReadWrite,
                automations: MacOsAutomationPermission::BundleIds(vec![
                    "com.apple.Calendar".to_string(),
                ]),
                launch_services: true,
                accessibility: true,
                calendar: false,
                reminders: true,
                contacts: MacOsContactsPermission::ReadOnly,
            }),
        };

        assert_eq!(
            additional_permission_profile_to_core(profile),
            PermissionProfile {
                network: Some(NetworkPermissions {
                    enabled: Some(true),
                }),
                file_system: Some(FileSystemPermissions {
                    read: Some(vec![
                        AbsolutePathBuf::from_absolute_path("/read").expect("absolute read path"),
                    ]),
                    write: Some(vec![
                        AbsolutePathBuf::from_absolute_path("/write").expect("absolute write path"),
                    ]),
                }),
                macos: Some(MacOsSeatbeltProfileExtensions {
                    macos_preferences: MacOsPreferencesPermission::ReadWrite,
                    macos_automation: MacOsAutomationPermission::BundleIds(vec![
                        "com.apple.Calendar".to_string(),
                    ]),
                    macos_launch_services: true,
                    macos_accessibility: true,
                    macos_calendar: false,
                    macos_reminders: true,
                    macos_contacts: MacOsContactsPermission::ReadOnly,
                }),
            }
        );
    }

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
            CancellationToken::new(),
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
                    network_access: false,
                },
                model: "mock-model".to_string(),
                service_tier: None,
                effort: None,
                summary: Some(codex_protocol::config_types::ReasoningSummary::Auto),
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
                        network_access: false,
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
                        skill_metadata: Some(
                            codex_app_server_protocol::CommandExecutionRequestApprovalSkillMetadata {
                                path_to_skills_md: PathBuf::from("/repo/SKILLS.md"),
                            },
                        ),
                        proposed_execpolicy_amendment: None,
                        proposed_network_policy_amendments: None,
                        available_decisions: None,
                    })
                    .expect("serialize exec approval params"),
                ),
                trace: None,
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
                assert_eq!(
                    request.skill_metadata,
                    Some(
                        codex_protocol::approvals::ExecApprovalRequestSkillMetadata {
                            path_to_skills_md: PathBuf::from("/repo/SKILLS.md"),
                        }
                    )
                );
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
                            skill_metadata: None,
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
            CancellationToken::new(),
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

    #[tokio::test]
    async fn set_thread_name_sends_thread_set_name_request() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/repo"),
            14,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);
        let (outbound_tx, mut outbound_rx) = unbounded_channel();
        let (codex_op_tx, codex_op_rx) = unbounded_channel();
        let task = tokio::spawn(op_task(
            codex_op_rx,
            outbound_tx,
            state.clone(),
            CancellationToken::new(),
            app_event_tx,
        ));

        codex_op_tx
            .send(Op::SetThreadName {
                name: "Renamed Thread".to_string(),
            })
            .expect("send set_thread_name op");
        drop(codex_op_tx);

        let outbound = timeout(Duration::from_secs(1), outbound_rx.recv())
            .await
            .expect("thread/name/set request should arrive")
            .expect("thread/name/set request should be present");
        task.await.expect("op task should finish cleanly");

        match outbound {
            JSONRPCMessage::Request(request) => {
                assert_eq!(request.id, RequestId::Integer(14));
                assert_eq!(request.method, "thread/name/set");
                let params: ThreadSetNameParams =
                    serde_json::from_value(request.params.expect("thread/name/set params"))
                        .expect("decode thread/name/set params");
                assert_eq!(
                    params,
                    ThreadSetNameParams {
                        thread_id: "thread-1".to_string(),
                        name: "Renamed Thread".to_string(),
                    }
                );
            }
            other => panic!("expected thread/name/set request, got {other:?}"),
        }

        let guard = state.lock().await;
        assert_eq!(guard.next_request_id, 15);
        drop(guard);

        assert!(matches!(
            app_event_rx.try_recv(),
            Err(TryRecvError::Empty | TryRecvError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn ignored_op_warning_names_the_action() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/repo"),
            1,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);
        let (outbound_tx, mut outbound_rx) = unbounded_channel();
        let (codex_op_tx, codex_op_rx) = unbounded_channel();
        let task = tokio::spawn(op_task(
            codex_op_rx,
            outbound_tx,
            state,
            CancellationToken::new(),
            app_event_tx,
        ));

        codex_op_tx.send(Op::Compact).expect("send compact op");
        drop(codex_op_tx);

        let event = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("warning event should arrive")
            .expect("warning event should be present");
        task.await.expect("op task should finish cleanly");

        match event {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::Warning(WarningEvent { message }),
                ..
            }) => {
                assert_eq!(message, "compact is currently ignored in connected mode");
            }
            other => panic!("expected warning event, got {other:?}"),
        }

        assert!(matches!(
            outbound_rx.try_recv(),
            Err(TryRecvError::Empty | TryRecvError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn fork_mode_sends_thread_fork_request_and_configures_session() {
        let cwd = tempdir().expect("temp cwd");
        let codex_home = tempdir().expect("temp codex home");
        let cwd_path = cwd.path().to_path_buf();
        let server_cwd_path = cwd_path.clone();
        let (client_io, server_io) = duplex(16 * 1024);
        let config = ConfigBuilder::default()
            .codex_home(codex_home.path().to_path_buf())
            .fallback_cwd(Some(cwd_path.clone()))
            .build()
            .await
            .expect("build config");
        let expected_model = config.model.clone();
        let expected_approval_policy: RemoteAskForApproval =
            config.permissions.approval_policy.value().into();

        let server = tokio::spawn(async move {
            let mut ws = WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;

            let request = ws
                .next()
                .await
                .expect("thread/fork request")
                .expect("websocket message")
                .into_text()
                .expect("request text");
            let request: JSONRPCRequest =
                serde_json::from_str(&request).expect("decode thread/fork request");
            assert_eq!(request.id, RequestId::Integer(2));
            assert_eq!(request.method, "thread/fork");

            let params: ThreadForkParams =
                serde_json::from_value(request.params.expect("thread/fork params"))
                    .expect("decode thread/fork params");
            assert_eq!(params.thread_id, "thread-source");
            assert_eq!(params.cwd, Some(server_cwd_path.display().to_string()));
            assert_eq!(params.model, expected_model);
            assert_eq!(params.approval_policy, Some(expected_approval_policy));

            let response = JSONRPCMessage::Response(JSONRPCResponse {
                id: request.id,
                result: serde_json::to_value(ThreadForkResponse {
                    thread: Thread {
                        id: ThreadId::new().to_string(),
                        preview: "forked preview".to_string(),
                        ephemeral: false,
                        model_provider: "openai".to_string(),
                        created_at: 10,
                        updated_at: 11,
                        status: ThreadStatus::Idle,
                        path: Some(PathBuf::from("/tmp/forked.jsonl")),
                        cwd: PathBuf::from("/remote/repo"),
                        cli_version: "0.106.0".to_string(),
                        source: RemoteSessionSource::Cli,
                        agent_nickname: None,
                        agent_role: None,
                        git_info: None,
                        name: Some("forked thread".to_string()),
                        turns: Vec::new(),
                    },
                    model: "mock-model".to_string(),
                    model_provider: "openai".to_string(),
                    service_tier: None,
                    cwd: PathBuf::from("/remote/repo"),
                    approval_policy: RemoteAskForApproval::OnFailure,
                    approvals_reviewer: RemoteApprovalsReviewer::User,
                    sandbox: codex_app_server_protocol::SandboxPolicy::WorkspaceWrite {
                        writable_roots: Vec::new(),
                        read_only_access: codex_app_server_protocol::ReadOnlyAccess::FullAccess,
                        network_access: false,
                        exclude_tmpdir_env_var: false,
                        exclude_slash_tmp: false,
                    },
                    reasoning_effort: None,
                })
                .expect("serialize thread/fork response"),
            });
            let response = serde_json::to_string(&response).expect("serialize json-rpc response");
            ws.send(WebSocketMessage::Text(response.into()))
                .await
                .expect("send thread/fork response");
        });
        let mut ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;

        let (thread_id, thread_cwd, event) = open_thread_for_session(
            &mut ws,
            &config,
            ConnectedSessionMode::Fork {
                thread_id: "thread-source".to_string(),
            },
        )
        .await
        .expect("fork session");

        server.await.expect("server task should finish");
        assert_eq!(thread_cwd, PathBuf::from("/remote/repo"));

        match event.msg {
            EventMsg::SessionConfigured(session) => {
                assert_eq!(session.session_id, thread_id);
                assert_eq!(session.thread_name, Some("forked thread".to_string()));
                assert_eq!(session.cwd, PathBuf::from("/remote/repo"));
                assert_eq!(
                    session.rollout_path,
                    Some(PathBuf::from("/tmp/forked.jsonl"))
                );
            }
            other => panic!("expected session configured event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn shutdown_sends_thread_unsubscribe_before_cancel() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/repo"),
            1,
        )));
        let (outbound_tx, mut outbound_rx) = unbounded_channel();
        let state_for_observer = state.clone();
        let observer = tokio::spawn(async move {
            let outbound = timeout(Duration::from_secs(1), outbound_rx.recv())
                .await
                .expect("unsubscribe request should arrive")
                .expect("unsubscribe request should be present");
            match outbound {
                JSONRPCMessage::Request(request) => {
                    assert_eq!(request.id, RequestId::Integer(1));
                    assert_eq!(request.method, "thread/unsubscribe");
                    let params: ThreadUnsubscribeParams =
                        serde_json::from_value(request.params.expect("unsubscribe params"))
                            .expect("decode unsubscribe params");
                    assert_eq!(params.thread_id, "thread-1");
                    state_for_observer
                        .lock()
                        .await
                        .pending_requests
                        .remove(&request.id);
                }
                other => panic!("expected unsubscribe request, got {other:?}"),
            }
        });

        ConnectedSessionHandle {
            cancel: CancellationToken::new(),
            outbound_tx,
            state,
            tasks: Vec::new(),
        }
        .shutdown()
        .await;

        observer.await.expect("observer task should finish");
    }

    #[test]
    fn resumed_thread_synthesizes_initial_messages() {
        let event = session_configured_event_from_thread_response(
            Thread {
                id: ThreadId::new().to_string(),
                preview: "hello".to_string(),
                ephemeral: false,
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
                                memory_citation: None,
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
                            memory_citation: None,
                        }],
                        status: TurnStatus::InProgress,
                        error: None,
                    },
                ],
            },
            "mock-model".to_string(),
            "openai".to_string(),
            None,
            PathBuf::from("/repo"),
            AskForApproval::Never,
            codex_protocol::config_types::ApprovalsReviewer::User,
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
    async fn reader_task_translates_command_stream_server_notifications() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/repo"),
            3,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);
        let (outbound_tx, _outbound_rx) = unbounded_channel();

        let messages = vec![
            JSONRPCMessage::Notification(JSONRPCNotification {
                method: "turn/started".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::TurnStartedNotification {
                        thread_id: "thread-1".to_string(),
                        turn: Turn {
                            id: "turn-1".to_string(),
                            items: Vec::new(),
                            status: TurnStatus::InProgress,
                            error: None,
                        },
                    })
                    .expect("serialize turn/started"),
                ),
            }),
            JSONRPCMessage::Notification(JSONRPCNotification {
                method: "item/started".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::ItemStartedNotification {
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        item: ThreadItem::CommandExecution {
                            id: "cmd-1".to_string(),
                            command: "echo hello".to_string(),
                            cwd: PathBuf::from("/repo"),
                            process_id: Some("proc-1".to_string()),
                            status: CommandExecutionStatus::InProgress,
                            command_actions: Vec::new(),
                            aggregated_output: None,
                            exit_code: None,
                            duration_ms: None,
                        },
                    })
                    .expect("serialize item/started"),
                ),
            }),
            JSONRPCMessage::Notification(JSONRPCNotification {
                method: "item/commandExecution/outputDelta".to_string(),
                params: Some(
                    serde_json::to_value(
                        codex_app_server_protocol::CommandExecutionOutputDeltaNotification {
                            thread_id: "thread-1".to_string(),
                            turn_id: "turn-1".to_string(),
                            item_id: "cmd-1".to_string(),
                            delta: "hello\n".to_string(),
                        },
                    )
                    .expect("serialize command output delta"),
                ),
            }),
            JSONRPCMessage::Notification(JSONRPCNotification {
                method: "item/completed".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::ItemCompletedNotification {
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        item: ThreadItem::CommandExecution {
                            id: "cmd-1".to_string(),
                            command: "echo hello".to_string(),
                            cwd: PathBuf::from("/repo"),
                            process_id: Some("proc-1".to_string()),
                            status: CommandExecutionStatus::Completed,
                            command_actions: Vec::new(),
                            aggregated_output: Some("hello\n".to_string()),
                            exit_code: Some(0),
                            duration_ms: Some(5),
                        },
                    })
                    .expect("serialize item/completed"),
                ),
            }),
            JSONRPCMessage::Notification(JSONRPCNotification {
                method: "turn/completed".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::TurnCompletedNotification {
                        thread_id: "thread-1".to_string(),
                        turn: Turn {
                            id: "turn-1".to_string(),
                            items: Vec::new(),
                            status: TurnStatus::Completed,
                            error: None,
                        },
                    })
                    .expect("serialize turn/completed"),
                ),
            }),
        ];
        let frames = futures::stream::iter(messages.into_iter().map(|message| {
            let payload = serde_json::to_string(&message).expect("serialize jsonrpc");
            Ok::<_, tokio_tungstenite::tungstenite::Error>(WebSocketMessage::Text(payload.into()))
        }));

        reader_task(
            frames,
            outbound_tx,
            state.clone(),
            CancellationToken::new(),
            app_event_tx,
        )
        .await;

        let first = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("turn started event should arrive")
            .expect("turn started app event should exist");
        match first {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::TurnStarted(TurnStartedEvent { turn_id, .. }),
                ..
            }) => {
                assert_eq!(turn_id, "turn-1");
            }
            other => panic!("expected TurnStarted event, got {other:?}"),
        }

        let second = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("exec begin event should arrive")
            .expect("exec begin app event should exist");
        match second {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::ExecCommandBegin(begin),
                ..
            }) => {
                assert_eq!(begin.call_id, "cmd-1");
                assert_eq!(begin.turn_id, "turn-1");
                assert_eq!(begin.command, vec!["echo".to_string(), "hello".to_string()]);
            }
            other => panic!("expected ExecCommandBegin event, got {other:?}"),
        }

        let third = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("exec delta event should arrive")
            .expect("exec delta app event should exist");
        match third {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::ExecCommandOutputDelta(delta),
                ..
            }) => {
                assert_eq!(delta.call_id, "cmd-1");
                assert_eq!(delta.chunk, b"hello\n".to_vec());
            }
            other => panic!("expected ExecCommandOutputDelta event, got {other:?}"),
        }

        let fourth = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("exec end event should arrive")
            .expect("exec end app event should exist");
        match fourth {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::ExecCommandEnd(end),
                ..
            }) => {
                assert_eq!(end.call_id, "cmd-1");
                assert_eq!(end.exit_code, 0);
                assert_eq!(end.aggregated_output, "hello\n");
            }
            other => panic!("expected ExecCommandEnd event, got {other:?}"),
        }

        let fifth = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("turn complete event should arrive")
            .expect("turn complete app event should exist");
        match fifth {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::TurnComplete(TurnCompleteEvent { turn_id, .. }),
                ..
            }) => {
                assert_eq!(turn_id, "turn-1");
            }
            other => panic!("expected TurnComplete event, got {other:?}"),
        }

        let guard = state.lock().await;
        assert_eq!(guard.current_turn_id, None);
    }

    #[tokio::test]
    async fn handle_notification_translates_agent_message_stream_notifications() {
        let thread_id = "00000000-0000-0000-0000-000000000001".to_string();
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            thread_id.clone(),
            PathBuf::from("/repo"),
            3,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);

        handle_notification(
            JSONRPCNotification {
                method: "item/agentMessage/delta".to_string(),
                params: Some(
                    serde_json::to_value(
                        codex_app_server_protocol::AgentMessageDeltaNotification {
                            thread_id: thread_id.clone(),
                            turn_id: "turn-1".to_string(),
                            item_id: "agent-1".to_string(),
                            delta: "hello".to_string(),
                        },
                    )
                    .expect("serialize agent delta notification"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        handle_notification(
            JSONRPCNotification {
                method: "item/completed".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::ItemCompletedNotification {
                        thread_id: thread_id.clone(),
                        turn_id: "turn-1".to_string(),
                        item: ThreadItem::AgentMessage {
                            id: "agent-1".to_string(),
                            text: "hello world".to_string(),
                            phase: None,
                            memory_citation: None,
                        },
                    })
                    .expect("serialize agent item completion"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        let first = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("agent delta event should arrive")
            .expect("agent delta app event should exist");
        match first {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::AgentMessageContentDelta(delta),
                ..
            }) => {
                assert_eq!(delta.turn_id, "turn-1");
                assert_eq!(delta.item_id, "agent-1");
                assert_eq!(delta.delta, "hello");
            }
            other => panic!("expected AgentMessageContentDelta, got {other:?}"),
        }

        let second = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("legacy agent delta event should arrive")
            .expect("legacy agent delta app event should exist");
        match second {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::AgentMessageDelta(delta),
                ..
            }) => {
                assert_eq!(delta.delta, "hello");
            }
            other => panic!("expected AgentMessageDelta, got {other:?}"),
        }

        let third = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("agent item completion should arrive")
            .expect("agent item completion app event should exist");
        match third {
            AppEvent::CodexEvent(Event {
                msg:
                    EventMsg::ItemCompleted(ItemCompletedEvent {
                        turn_id,
                        item: TurnItem::AgentMessage(item),
                        ..
                    }),
                ..
            }) => {
                assert_eq!(turn_id, "turn-1");
                assert_eq!(item.id, "agent-1");
                assert_eq!(item.content.len(), 1);
                match &item.content[0] {
                    AgentMessageContent::Text { text } => {
                        assert_eq!(text, "hello world");
                    }
                }
            }
            other => panic!("expected ItemCompleted(agent message), got {other:?}"),
        }

        let fourth = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("legacy agent message event should arrive")
            .expect("legacy agent message app event should exist");
        match fourth {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::AgentMessage(message),
                ..
            }) => {
                assert_eq!(message.message, "hello world");
            }
            other => panic!("expected AgentMessage event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn turn_complete_includes_last_agent_message_for_connected_notifications() {
        let thread_id = "00000000-0000-0000-0000-000000000011".to_string();
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            thread_id.clone(),
            PathBuf::from("/repo"),
            3,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);

        handle_notification(
            JSONRPCNotification {
                method: "turn/started".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::TurnStartedNotification {
                        thread_id: thread_id.clone(),
                        turn: Turn {
                            id: "turn-1".to_string(),
                            items: Vec::new(),
                            error: None,
                            status: TurnStatus::InProgress,
                        },
                    })
                    .expect("serialize turn started notification"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        handle_notification(
            JSONRPCNotification {
                method: "item/agentMessage/delta".to_string(),
                params: Some(
                    serde_json::to_value(
                        codex_app_server_protocol::AgentMessageDeltaNotification {
                            thread_id: thread_id.clone(),
                            turn_id: "turn-1".to_string(),
                            item_id: "agent-1".to_string(),
                            delta: "hello".to_string(),
                        },
                    )
                    .expect("serialize agent delta notification"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        handle_notification(
            JSONRPCNotification {
                method: "item/completed".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::ItemCompletedNotification {
                        thread_id: thread_id.clone(),
                        turn_id: "turn-1".to_string(),
                        item: ThreadItem::AgentMessage {
                            id: "agent-1".to_string(),
                            text: "hello world".to_string(),
                            phase: None,
                            memory_citation: None,
                        },
                    })
                    .expect("serialize agent item completion"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        handle_notification(
            JSONRPCNotification {
                method: "turn/completed".to_string(),
                params: Some(
                    serde_json::to_value(TurnCompletedNotification {
                        thread_id,
                        turn: Turn {
                            id: "turn-1".to_string(),
                            items: Vec::new(),
                            error: None,
                            status: TurnStatus::Completed,
                        },
                    })
                    .expect("serialize turn completed notification"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        let mut turn_complete = None;
        for _ in 0..6 {
            let event = timeout(Duration::from_secs(1), app_event_rx.recv())
                .await
                .expect("app event should arrive")
                .expect("app event should exist");
            if let AppEvent::CodexEvent(Event {
                msg: EventMsg::TurnComplete(event),
                ..
            }) = event
            {
                turn_complete = Some(event);
                break;
            }
        }

        let turn_complete = turn_complete.expect("turn complete event should be emitted");
        assert_eq!(turn_complete.turn_id, "turn-1");
        assert_eq!(
            turn_complete.last_agent_message,
            Some("hello world".to_string())
        );

        let guard = state.lock().await;
        assert_eq!(guard.current_turn_id, None);
        assert_eq!(guard.current_turn_last_agent_message, None);
    }

    #[tokio::test]
    async fn handle_notification_translates_reasoning_stream_notifications() {
        let thread_id = "00000000-0000-0000-0000-000000000002".to_string();
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            thread_id.clone(),
            PathBuf::from("/repo"),
            3,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);

        handle_notification(
            JSONRPCNotification {
                method: "item/reasoning/summaryTextDelta".to_string(),
                params: Some(
                    serde_json::to_value(
                        codex_app_server_protocol::ReasoningSummaryTextDeltaNotification {
                            thread_id: thread_id.clone(),
                            turn_id: "turn-1".to_string(),
                            item_id: "reason-1".to_string(),
                            delta: "thinking".to_string(),
                            summary_index: 0,
                        },
                    )
                    .expect("serialize reasoning summary delta"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        handle_notification(
            JSONRPCNotification {
                method: "item/reasoning/textDelta".to_string(),
                params: Some(
                    serde_json::to_value(
                        codex_app_server_protocol::ReasoningTextDeltaNotification {
                            thread_id: thread_id.clone(),
                            turn_id: "turn-1".to_string(),
                            item_id: "reason-1".to_string(),
                            delta: "raw".to_string(),
                            content_index: 0,
                        },
                    )
                    .expect("serialize reasoning raw delta"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        handle_notification(
            JSONRPCNotification {
                method: "item/completed".to_string(),
                params: Some(
                    serde_json::to_value(codex_app_server_protocol::ItemCompletedNotification {
                        thread_id: thread_id.clone(),
                        turn_id: "turn-1".to_string(),
                        item: ThreadItem::Reasoning {
                            id: "reason-1".to_string(),
                            summary: vec!["thinking complete".to_string()],
                            content: vec!["raw complete".to_string()],
                        },
                    })
                    .expect("serialize reasoning completion"),
                ),
            },
            &state,
            &app_event_tx,
        )
        .await;

        let first = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("reasoning content delta should arrive")
            .expect("reasoning content delta app event should exist");
        match first {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::ReasoningContentDelta(delta),
                ..
            }) => {
                assert_eq!(delta.turn_id, "turn-1");
                assert_eq!(delta.item_id, "reason-1");
                assert_eq!(delta.delta, "thinking");
                assert_eq!(delta.summary_index, 0);
            }
            other => panic!("expected ReasoningContentDelta, got {other:?}"),
        }

        let second = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("legacy reasoning delta should arrive")
            .expect("legacy reasoning delta app event should exist");
        match second {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::AgentReasoningDelta(delta),
                ..
            }) => {
                assert_eq!(delta.delta, "thinking");
            }
            other => panic!("expected AgentReasoningDelta, got {other:?}"),
        }

        let third = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("reasoning raw delta should arrive")
            .expect("reasoning raw delta app event should exist");
        match third {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::ReasoningRawContentDelta(delta),
                ..
            }) => {
                assert_eq!(delta.turn_id, "turn-1");
                assert_eq!(delta.item_id, "reason-1");
                assert_eq!(delta.delta, "raw");
                assert_eq!(delta.content_index, 0);
            }
            other => panic!("expected ReasoningRawContentDelta, got {other:?}"),
        }

        let fourth = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("legacy raw reasoning delta should arrive")
            .expect("legacy raw reasoning delta app event should exist");
        match fourth {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::AgentReasoningRawContentDelta(delta),
                ..
            }) => {
                assert_eq!(delta.delta, "raw");
            }
            other => panic!("expected AgentReasoningRawContentDelta, got {other:?}"),
        }

        let fifth = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("reasoning item completion should arrive")
            .expect("reasoning item completion app event should exist");
        match fifth {
            AppEvent::CodexEvent(Event {
                msg:
                    EventMsg::ItemCompleted(ItemCompletedEvent {
                        turn_id,
                        item: TurnItem::Reasoning(item),
                        ..
                    }),
                ..
            }) => {
                assert_eq!(turn_id, "turn-1");
                assert_eq!(item.id, "reason-1");
                assert_eq!(item.summary_text, vec!["thinking complete".to_string()]);
                assert_eq!(item.raw_content, vec!["raw complete".to_string()]);
            }
            other => panic!("expected ItemCompleted(reasoning), got {other:?}"),
        }

        let sixth = timeout(Duration::from_secs(1), app_event_rx.recv())
            .await
            .expect("final reasoning summary event should arrive")
            .expect("final reasoning summary app event should exist");
        match sixth {
            AppEvent::CodexEvent(Event {
                msg: EventMsg::AgentReasoning(reasoning),
                ..
            }) => {
                assert_eq!(reasoning.text, "thinking complete");
            }
            other => panic!("expected AgentReasoning event, got {other:?}"),
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
            CancellationToken::new(),
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

    #[tokio::test]
    async fn cancelled_disconnect_does_not_request_fatal_exit() {
        let state = Arc::new(Mutex::new(ConnectedSessionState::new(
            "thread-1".to_string(),
            PathBuf::from("/repo"),
            3,
        )));
        let (app_event_tx_raw, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx_raw);
        let (outbound_tx, _outbound_rx) = unbounded_channel();
        let cancel = CancellationToken::new();
        cancel.cancel();

        reader_task(
            futures::stream::empty::<
                std::result::Result<WebSocketMessage, tokio_tungstenite::tungstenite::Error>,
            >(),
            outbound_tx,
            state,
            cancel,
            app_event_tx,
        )
        .await;

        assert!(matches!(
            app_event_rx.try_recv(),
            Err(TryRecvError::Empty | TryRecvError::Disconnected)
        ));
    }
}
