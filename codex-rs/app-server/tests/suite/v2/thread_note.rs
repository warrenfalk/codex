use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_shell_command_sse_response;
use app_test_support::to_response;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadNoteCreateParams;
use codex_app_server_protocol::ThreadNoteCreateResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput as V2UserInput;
use codex_core::RolloutRecorder;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::RolloutItem;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use std::path::Path;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test]
async fn thread_note_create_persists_note_and_emits_idle_display_turn() -> Result<()> {
    let server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri())?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let thread = start_thread(&mut mcp).await?;

    let note_req = mcp
        .send_thread_note_create_request(ThreadNoteCreateParams {
            thread_id: thread.id.clone(),
            note: "  remember this\nexactly  ".to_string(),
        })
        .await?;
    let note_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(note_req)),
    )
    .await??;
    let note_response = to_response::<ThreadNoteCreateResponse>(note_resp)?;

    assert_eq!(
        note_response.item,
        ThreadItem::NoteToSelf {
            id: "item-1".to_string(),
            note: "remember this\nexactly".to_string(),
        }
    );

    let completed_notification = timeout(DEFAULT_READ_TIMEOUT, async {
        let notification = mcp
            .read_stream_until_notification_message("turn/completed")
            .await?;
        let params = notification
            .params
            .context("turn/completed params missing")?;
        serde_json::from_value::<TurnCompletedNotification>(params)
            .context("deserialize turn/completed")
    })
    .await??;
    assert_eq!(completed_notification.thread_id, thread.id);
    assert_eq!(completed_notification.turn.id, note_response.turn_id);
    assert_eq!(completed_notification.turn.status, TurnStatus::Completed);
    assert_eq!(
        completed_notification.turn.items,
        vec![note_response.item.clone()]
    );

    let read_req = mcp
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread.id.clone(),
            include_turns: true,
        })
        .await?;
    let read_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(read_req)),
    )
    .await??;
    let ThreadReadResponse {
        thread: read_thread,
    } = to_response::<ThreadReadResponse>(read_resp)?;
    assert_eq!(read_thread.name, None);
    assert_eq!(read_thread.preview, "");
    assert_eq!(read_thread.turns.len(), 1);
    assert_eq!(read_thread.turns[0].items, vec![note_response.item]);

    let rollout_path = thread.path.as_ref().context("thread path missing")?;
    let history = RolloutRecorder::get_rollout_history(rollout_path).await?;
    let InitialHistory::Resumed(resumed_history) = history else {
        panic!("expected resumed rollout history");
    };
    assert!(
        resumed_history.history.iter().any(|item| {
            matches!(
                item,
                RolloutItem::EventMsg(EventMsg::NoteToSelf(event))
                    if event.note == "remember this\nexactly"
            )
        }),
        "note should be persisted as an EventMsg::NoteToSelf"
    );

    Ok(())
}

#[tokio::test]
async fn thread_note_create_appends_to_active_turn_and_emits_item_completed() -> Result<()> {
    let server = responses::start_mock_server().await;
    let _response_mock = responses::mount_sse_once(
        &server,
        create_shell_command_sse_response(
            vec![
                "python3".to_string(),
                "-c".to_string(),
                "print(42)".to_string(),
            ],
            /*workdir*/ None,
            Some(5000),
            "call-note",
        )?,
    )
    .await;
    let codex_home = TempDir::new()?;
    create_config_toml_with_approval_policy(codex_home.path(), &server.uri(), "untrusted")?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let thread = start_thread(&mut mcp).await?;
    let turn_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "ask me".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turn_req)),
    )
    .await??;
    let TurnStartResponse { turn } = to_response::<TurnStartResponse>(turn_resp)?;

    let server_req = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_request_message(),
    )
    .await??;
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = server_req else {
        panic!("expected CommandExecutionRequestApproval request, got: {server_req:?}");
    };
    assert_eq!(params.thread_id, thread.id);
    assert_eq!(params.turn_id, turn.id);

    let note_req = mcp
        .send_thread_note_create_request(ThreadNoteCreateParams {
            thread_id: thread.id.clone(),
            note: "during turn".to_string(),
        })
        .await?;
    let note_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(note_req)),
    )
    .await??;
    let note_response = to_response::<ThreadNoteCreateResponse>(note_resp)?;

    assert_eq!(note_response.turn_id, turn.id);
    let ThreadItem::NoteToSelf { note, .. } = &note_response.item else {
        panic!(
            "expected noteToSelf response item, got: {:?}",
            note_response.item
        );
    };
    assert_eq!(note, "during turn");

    let completed_notification = timeout(DEFAULT_READ_TIMEOUT, async {
        let notification = mcp
            .read_stream_until_matching_notification("note item/completed", |notification| {
                notification.method == "item/completed"
                    && notification
                        .params
                        .as_ref()
                        .and_then(|params| params.get("item"))
                        .and_then(|item| item.get("type"))
                        .and_then(|kind| kind.as_str())
                        == Some("noteToSelf")
            })
            .await?;
        let params = notification
            .params
            .context("item/completed params missing")?;
        serde_json::from_value::<ItemCompletedNotification>(params)
            .context("deserialize item/completed")
    })
    .await??;
    assert_eq!(completed_notification.thread_id, thread.id);
    assert_eq!(completed_notification.turn_id, turn.id);
    assert_eq!(completed_notification.item, note_response.item);

    let turns_req = mcp
        .send_thread_turns_list_request(ThreadTurnsListParams {
            thread_id: thread.id.clone(),
            cursor: None,
            limit: Some(1),
            sort_direction: Some(SortDirection::Desc),
            items_view: Some(TurnItemsView::Full),
        })
        .await?;
    let turns_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turns_req)),
    )
    .await??;
    let ThreadTurnsListResponse { data, .. } = to_response::<ThreadTurnsListResponse>(turns_resp)?;
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].id, turn.id);
    assert!(
        data[0].items.iter().any(|item| item == &note_response.item),
        "expected active turn history to include note: {:?}",
        data[0].items
    );

    Ok(())
}

#[tokio::test]
async fn thread_note_create_rejects_empty_note() -> Result<()> {
    let server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri())?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let thread = start_thread(&mut mcp).await?;

    let note_req = mcp
        .send_thread_note_create_request(ThreadNoteCreateParams {
            thread_id: thread.id,
            note: " \n\t ".to_string(),
        })
        .await?;
    let error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(note_req)),
    )
    .await??;

    assert_eq!(error.error.code, -32602);
    assert_eq!(error.error.message, "note must not be empty");

    Ok(())
}

async fn start_thread(mcp: &mut TestAppServer) -> Result<codex_app_server_protocol::Thread> {
    let start_req = mcp
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let start_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(start_req)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response::<ThreadStartResponse>(start_resp)?;
    Ok(thread)
}

fn create_config_toml(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    create_config_toml_with_approval_policy(codex_home, server_uri, "never")
}

fn create_config_toml_with_approval_policy(
    codex_home: &Path,
    server_uri: &str,
    approval_policy: &str,
) -> std::io::Result<()> {
    let config_toml = codex_home.join("config.toml");
    std::fs::write(
        config_toml,
        format!(
            r#"
model = "mock-model"
approval_policy = "{approval_policy}"
sandbox_mode = "read-only"

model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
}
