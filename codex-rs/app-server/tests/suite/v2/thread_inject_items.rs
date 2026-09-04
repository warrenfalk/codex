use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::AdditionalContextEntry;
use codex_app_server_protocol::AdditionalContextKind;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadInjectItemsParams;
use codex_app_server_protocol::ThreadInjectItemsResponse;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::TurnToolOutput;
use codex_app_server_protocol::UserInput as V2UserInput;
use codex_core::RolloutRecorder;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::ResponseItem;
use codex_rollout::InitialHistory;
use codex_rollout::RolloutItem;
use codex_state::StateRuntime;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use core_test_support::responses::strip_response_item_id;
use core_test_support::responses::strip_response_item_ids_from_json;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use tempfile::TempDir;
use test_case::test_case;
use tokio::time::Duration;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test]
async fn thread_inject_items_notifies_every_subscribed_connection() -> Result<()> {
    use super::connection_handling_websocket::assert_no_message;
    use super::connection_handling_websocket::connect_websocket;
    use super::connection_handling_websocket::read_jsonrpc_message;
    use super::connection_handling_websocket::read_notification_for_method;
    use super::connection_handling_websocket::read_response_and_notification_for_method;
    use super::connection_handling_websocket::read_response_for_id;
    use super::connection_handling_websocket::send_initialize_request;
    use super::connection_handling_websocket::send_request;
    use super::connection_handling_websocket::spawn_websocket_server;
    use super::connection_handling_websocket::start_thread;

    let server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(codex_home.path())?;
    let (mut process, bind_addr) = spawn_websocket_server(codex_home.path()).await?;

    let mut originating_client = connect_websocket(bind_addr).await?;
    let mut observing_client = connect_websocket(bind_addr).await?;
    send_initialize_request(&mut originating_client, /*id*/ 1, "originating_client").await?;
    read_response_for_id(&mut originating_client, /*id*/ 1)
        .await
        .context("waiting for originating client initialization")?;
    send_initialize_request(&mut observing_client, /*id*/ 2, "observing_client").await?;
    read_response_for_id(&mut observing_client, /*id*/ 2)
        .await
        .context("waiting for observing client initialization")?;

    let thread_id = start_thread(&mut originating_client, /*id*/ 3)
        .await
        .context("starting shared thread")?;
    send_request(
        &mut originating_client,
        "thread/inject_items",
        /*id*/ 4,
        Some(serde_json::to_value(ThreadInjectItemsParams {
            thread_id: thread_id.clone(),
            items: vec![serde_json::to_value(ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "Existing parent context".to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            })?],
        })?),
    )
    .await?;
    let materialize_response = read_response_for_id(&mut originating_client, /*id*/ 4)
        .await
        .context("materializing shared thread rollout")?;
    let _: ThreadInjectItemsResponse = to_response(materialize_response)?;

    send_request(
        &mut observing_client,
        "thread/resume",
        /*id*/ 5,
        Some(serde_json::to_value(ThreadResumeParams {
            thread_id: thread_id.clone(),
            ..Default::default()
        })?),
    )
    .await?;
    let resume_response = loop {
        match read_jsonrpc_message(&mut observing_client)
            .await
            .context("subscribing observing client to shared thread")?
        {
            JSONRPCMessage::Response(response) if response.id == RequestId::Integer(5) => {
                break response;
            }
            JSONRPCMessage::Error(error) if error.id == RequestId::Integer(5) => {
                bail!(
                    "observing client thread/resume failed: {}",
                    error.error.message
                );
            }
            _ => {}
        }
    };
    let _: ThreadResumeResponse = to_response(resume_response)?;

    let summary = "Side conversation summary\n\nFindings from side chat.";
    let injected_item = ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: summary.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    send_request(
        &mut originating_client,
        "thread/inject_items",
        /*id*/ 6,
        Some(serde_json::to_value(ThreadInjectItemsParams {
            thread_id: thread_id.clone(),
            items: vec![serde_json::to_value(injected_item)?],
        })?),
    )
    .await?;

    let item_notification = read_notification_for_method(&mut observing_client, "item/completed")
        .await
        .context("waiting for observing client item/completed notification")?;
    let item: ItemCompletedNotification = serde_json::from_value(
        item_notification
            .params
            .context("item/completed notification must include params")?,
    )?;
    assert_no_message(&mut observing_client, Duration::from_millis(250)).await?;

    let (inject_response, originating_item_notification) =
        read_response_and_notification_for_method(
            &mut originating_client,
            /*id*/ 6,
            "item/completed",
        )
        .await
        .context("waiting for originating client injection response and item notification")?;
    let _: ThreadInjectItemsResponse = to_response(inject_response)?;
    let originating_item: ItemCompletedNotification = serde_json::from_value(
        originating_item_notification
            .params
            .context("originating item/completed notification must include params")?,
    )?;

    assert_eq!(item.thread_id, thread_id);
    assert_eq!(originating_item, item);
    assert_eq!(
        item.item,
        ThreadItem::AgentMessage {
            id: "item-1".to_string(),
            text: summary.to_string(),
            phase: None,
            memory_citation: None,
            delivery: None,
            questions: None,
        }
    );

    process.kill().await?;
    Ok(())
}

#[tokio::test]
async fn thread_inject_items_stays_silent_for_hidden_context_messages() -> Result<()> {
    let server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(codex_home.path())?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized_with_timeout(DEFAULT_READ_TIMEOUT)
        .await?;

    let thread_req = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(thread_req)).await??;
    mcp.clear_message_buffer();

    let inject_req = mcp
        .send_thread_inject_items_request(ThreadInjectItemsParams {
            thread_id: thread.id,
            items: vec![serde_json::to_value(ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Side conversation boundary".to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            })?],
        })
        .await?;
    let _: ThreadInjectItemsResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(inject_req)).await??;

    let lifecycle_notifications = mcp
        .pending_notification_methods()
        .into_iter()
        .filter(|method| method == "item/completed" || method == "turn/completed")
        .collect::<Vec<_>>();
    assert_eq!(lifecycle_notifications, Vec::<String>::new());

    Ok(())
}

#[test_case(ThreadHistoryMode::Legacy; "legacy")]
#[test_case(ThreadHistoryMode::Paginated; "paginated")]
#[tokio::test]
async fn thread_inject_items_adds_raw_response_items_to_thread_history(
    history_mode: ThreadHistoryMode,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let body = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_assistant_message("msg-1", "Done"),
        responses::ev_completed("resp-1"),
    ]);
    let response_mock = responses::mount_sse_once(&server, body).await;

    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::Sqlite)
        .enable_feature(Feature::RetainClientDeveloperMessages)
        .with_extra_config("[memories]\ndisable_on_external_context = true")
        .write(codex_home.path())?;
    let state_db = StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(codex_home.path().abs()),
        "mock_provider".into(),
    )
    .await?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized_with_timeout(DEFAULT_READ_TIMEOUT)
        .await?;

    let thread_req = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            history_mode: Some(history_mode),
            ..Default::default()
        })
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(thread_req)).await??;

    let injected_text = "Injected assistant context";
    let injected_item = ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: injected_text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let developer_item = |text: &str| ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let injected_developer_item = developer_item("Injected developer context");
    let marker_shaped_developer_item =
        developer_item("<image_resize_notice>\nclient message\n</image_resize_notice>");
    let named_tool_output = json!({
        "type": "function_call_output",
        "name": "send_message_to_thread",
        "namespace": "codex_app",
        "output": "Another agent delegated this task.",
    });
    let named_tool_item: ResponseItem = serde_json::from_value(named_tool_output.clone())?;

    let inject_req = mcp
        .send_thread_inject_items_request(ThreadInjectItemsParams {
            thread_id: thread.id.clone(),
            items: vec![
                serde_json::to_value(&injected_item)?,
                serde_json::to_value(&injected_developer_item)?,
                serde_json::to_value(&marker_shaped_developer_item)?,
                named_tool_output.clone(),
            ],
        })
        .await?;
    let _response: ThreadInjectItemsResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(inject_req)).await??;
    assert!(response_mock.requests().is_empty());

    assert_eq!(
        state_db
            .get_thread_memory_mode(ThreadId::from_string(&thread.id)?)
            .await?
            .as_deref(),
        Some("polluted")
    );

    let rollout_path = thread.path.as_ref().context("thread path missing")?;
    let history = RolloutRecorder::get_rollout_history(rollout_path).await?;
    let InitialHistory::Resumed(resumed_history) = history else {
        panic!("expected resumed rollout history");
    };
    let persisted_injected_items = resumed_history
        .history
        .iter()
        .filter_map(|item| match item {
            RolloutItem::ResponseItem(envelope) => Some((
                strip_response_item_id(responses::strip_metadata(envelope.item.clone())),
                envelope
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.client_authored),
            )),
            _ => None,
        })
        .filter(|(item, _)| {
            item == &injected_item
                || item == &injected_developer_item
                || item == &marker_shaped_developer_item
                || item == &named_tool_item
        })
        .collect::<Vec<_>>();
    assert_eq!(
        persisted_injected_items,
        vec![
            (injected_item.clone(), None),
            (injected_developer_item.clone(), Some(true)),
            (marker_shaped_developer_item.clone(), Some(true)),
            (named_tool_item, None),
        ]
    );

    let application_context_text = "Application developer context";
    let untrusted_context_text = "Untrusted client context";
    let turn_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "Hello".to_string(),
                text_elements: Vec::new(),
            }],
            additional_context: Some(HashMap::from([
                (
                    "application_context".to_string(),
                    AdditionalContextEntry {
                        value: application_context_text.to_string(),
                        kind: AdditionalContextKind::Application,
                    },
                ),
                (
                    "untrusted_context".to_string(),
                    AdditionalContextEntry {
                        value: untrusted_context_text.to_string(),
                        kind: AdditionalContextKind::Untrusted,
                    },
                ),
            ])),
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse = timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(turn_req)).await??;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let InitialHistory::Resumed(resumed_history) =
        RolloutRecorder::get_rollout_history(rollout_path).await?
    else {
        panic!("expected resumed rollout history");
    };
    let persisted_additional_context = resumed_history
        .history
        .iter()
        .filter_map(|item| match item {
            RolloutItem::ResponseItem(envelope) => {
                let ResponseItem::Message { role, content, .. } = &envelope.item else {
                    return None;
                };
                content.iter().find_map(|item| {
                    let ContentItem::InputText { text } = item else {
                        return None;
                    };
                    (text.contains(application_context_text)
                        || text.contains(untrusted_context_text))
                    .then(|| {
                        (
                            role.as_str(),
                            envelope
                                .metadata
                                .as_ref()
                                .map(|metadata| metadata.client_authored),
                        )
                    })
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        persisted_additional_context,
        vec![("developer", Some(true)), ("user", None)]
    );

    let injected_value = serde_json::to_value(&injected_item)?;
    let model_input: Vec<Value> = response_mock
        .single_request()
        .input()
        .into_iter()
        .map(strip_response_item_ids_from_json)
        .collect();
    assert!(
        model_input
            .iter()
            .all(|item| item.get("metadata").is_none() && item.get("client_authored").is_none()),
        "private harness metadata must never enter the provider request"
    );
    assert!(
        response_item_text_position(&model_input, application_context_text).is_some(),
        "application-provided developer context should reach the model"
    );
    let environment_context_index =
        response_item_text_position(&model_input, "<environment_context>")
            .expect("environment context should be injected before the first user turn");
    let injected_index = model_input
        .iter()
        .position(|item| item == &injected_value)
        .expect("injected item should be sent in the next model request");
    let user_prompt_index = response_item_text_position(&model_input, "Hello")
        .expect("user prompt should be sent in the next model request");
    assert!(
        environment_context_index < injected_index,
        "standard initial context should be sent before injected items"
    );
    assert!(
        injected_index < user_prompt_index,
        "injected items should be sent before the user prompt"
    );
    assert!(
        model_input.contains(&named_tool_output),
        "named unpaired tool output should be sent in the next model request"
    );

    let delegated_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![responses::ev_completed("resp-delegated")]),
    )
    .await;
    mcp.clear_message_buffer();
    let delegated_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            input: Vec::new(),
            tool_output: Some(Box::new(TurnToolOutput {
                name: "send_message_to_thread".to_string(),
                namespace: Some("codex_app".to_string()),
                output: FunctionCallOutputBody::Text("Start a delegated turn.".to_string()),
            })),
            ..Default::default()
        })
        .await?;
    let TurnStartResponse { turn } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(delegated_req)).await??;
    let turn_started: TurnStartedNotification =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_notification("turn/started")).await??;
    let item_started: ItemStartedNotification =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_notification("item/started")).await??;
    let item_completed: ItemCompletedNotification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_notification("item/completed"),
    )
    .await??;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    assert_eq!(turn.id, turn_started.turn.id);
    assert_eq!(turn_started.turn.status, TurnStatus::InProgress);
    assert_eq!(item_completed.turn_id, turn_started.turn.id);
    assert_eq!(item_started.item, item_completed.item);
    assert!(matches!(
        &item_completed.item,
        ThreadItem::FunctionCallOutput {
            name,
            namespace,
            output: FunctionCallOutputBody::Text(output),
            ..
        } if name == "send_message_to_thread"
            && namespace.as_deref() == Some("codex_app")
            && output == "Start a delegated turn."
    ));
    assert!(
        delegated_mock
            .single_request()
            .input()
            .into_iter()
            .any(|item| item["type"] == "function_call_output"
                && item["output"] == "Start a delegated turn."),
        "the delegated turn must preserve the original model-visible tool output"
    );
    let resume_req = mcp
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id,
            ..Default::default()
        })
        .await?;
    let ThreadResumeResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(resume_req)).await??;
    assert_eq!(thread.turns.len(), 2);
    assert!(thread.turns.iter().any(|turn| {
        turn.id == turn_started.turn.id
            && turn.status == TurnStatus::Completed
            && turn.items.contains(&item_completed.item)
    }));

    Ok(())
}

#[tokio::test]
async fn thread_inject_items_adds_raw_response_items_after_a_turn() -> Result<()> {
    let server = responses::start_mock_server().await;
    let first_body = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_assistant_message("msg-1", "First done"),
        responses::ev_completed("resp-1"),
    ]);
    let second_body = responses::sse(vec![
        responses::ev_response_created("resp-2"),
        responses::ev_assistant_message("msg-2", "Second done"),
        responses::ev_completed("resp-2"),
    ]);
    let response_mock = responses::mount_sse_sequence(&server, vec![first_body, second_body]).await;

    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(codex_home.path())?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized_with_timeout(DEFAULT_READ_TIMEOUT)
        .await?;

    let thread_req = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(thread_req)).await??;

    let first_turn_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "First turn".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(first_turn_req)).await??;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let injected_item = ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: "Injected after first turn".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let injected_value = serde_json::to_value(&injected_item)?;

    let inject_req = mcp
        .send_thread_inject_items_request(ThreadInjectItemsParams {
            thread_id: thread.id.clone(),
            items: vec![injected_value.clone()],
        })
        .await?;
    let _response: ThreadInjectItemsResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(inject_req)).await??;

    let rollout_path = thread.path.as_ref().context("thread path missing")?;
    let InitialHistory::Resumed(resumed_history) =
        RolloutRecorder::get_rollout_history(rollout_path).await?
    else {
        panic!("expected resumed rollout history");
    };
    let persisted_developer_item = resumed_history
        .history
        .iter()
        .find_map(|item| match item {
            RolloutItem::ResponseItem(envelope)
                if strip_response_item_id(responses::strip_metadata(envelope.item.clone()))
                    == injected_item =>
            {
                Some(envelope)
            }
            _ => None,
        })
        .context("injected developer item should be persisted")?;
    assert_eq!(persisted_developer_item.metadata, None);

    let second_turn_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "Second turn".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(second_turn_req)).await??;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        !requests[0]
            .input()
            .into_iter()
            .map(strip_response_item_ids_from_json)
            .any(|item| item == injected_value),
        "injected item should not be sent before it is injected"
    );
    assert!(
        requests[1]
            .input()
            .into_iter()
            .map(strip_response_item_ids_from_json)
            .any(|item| item == injected_value),
        "injected item should be sent after being injected into existing history"
    );

    Ok(())
}

fn response_item_text_position(items: &[Value], needle: &str) -> Option<usize> {
    items.iter().position(|item| {
        item.get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|content| {
                content
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| text.contains(needle))
            })
    })
}
