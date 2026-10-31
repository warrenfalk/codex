//! Virtual-clock coverage of approval deadlines and stale timer cancellation.
use super::*;
use crate::message_processor::message_processor_tracing_tests::build_test_processor;
use crate::message_processor::message_processor_tracing_tests::read_response_from;
use crate::outgoing_message::ConnectionId;
use crate::transport::AppServerTransport;
use crate::transport::ConnectionOrigin;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::*;
use codex_core::config::ConfigBuilder;
use codex_login::AuthManager;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

struct Fixture {
    processor: Arc<MessageProcessor>,
    session: Arc<ConnectionSessionState>,
    id: String,
    _home: TempDir,
    _server: wiremock::MockServer,
    drain: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new() -> Result<Self> {
        let server = create_mock_responses_server_repeating_assistant("Done").await;
        let home = TempDir::new()?;
        MockResponsesConfig::new(&server.uri()).write(home.path())?;
        let config = Arc::new(
            ConfigBuilder::default()
                .codex_home(home.path().to_path_buf())
                .build()
                .await?,
        );
        let auth = AuthManager::shared_from_config(
            config.as_ref(),
            /*enable_codex_api_key_env*/ false,
        )
        .await?;
        let (processor, mut outgoing) = build_test_processor(config, auth).await;
        let session = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        let connection = ConnectionId(1);
        for request in [
            json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"human","version":"1"}}}),
            json!({"id":2,"method":"thread/start","params":{"ephemeral":true}}),
        ] {
            let id = request["id"].as_i64().expect("numeric request ID");
            processor
                .process_request(
                    connection,
                    serde_json::from_value(request)?,
                    &AppServerTransport::WebSocket {
                        bind_address: "127.0.0.1:0".parse()?,
                    },
                    Arc::clone(&session),
                )
                .await;
            if id == 1 {
                let _: InitializeResponse = read_response_from(&mut outgoing, connection, id).await;
                // A raw processor harness has no websocket transport to register the live client.
                processor
                    .connection_initialized(connection, /*request_attestation*/ false)
                    .await;
            } else {
                let started: ThreadStartResponse =
                    read_response_from(&mut outgoing, connection, id).await;
                let drain = tokio::spawn(async move { while outgoing.recv().await.is_some() {} });
                return Ok(Self {
                    processor,
                    session,
                    id: started.thread.id,
                    _home: home,
                    _server: server,
                    drain,
                });
            }
        }
        unreachable!()
    }

    async fn request(&self, request_id: &str) -> Result<ThreadControlState> {
        self.processor
            .thread_control
            .request(
                "dot",
                ThreadControlRequestParams {
                    thread_id: self.id.clone(),
                    request_id: request_id.to_owned(),
                    reason: "Continue".to_owned(),
                },
            )
            .await
            .map_err(|error| anyhow::anyhow!("{error:?}"))
    }

    async fn read(&self) -> Result<ThreadControlState> {
        let (id, entry) = self
            .processor
            .thread_control
            .entry(&self.id)
            .await
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        Ok(self.processor.thread_control.snapshot(id, &entry).await)
    }

    async fn update(&self, action: ThreadControlAction) -> Result<ThreadControlState> {
        self.processor
            .thread_control
            .update(ThreadControlUpdateParams {
                thread_id: self.id.clone(),
                action,
            })
            .await
            .map_err(|error| anyhow::anyhow!("{error:?}"))
    }

    async fn wait_for_grant(&self) -> Result<ThreadControlState> {
        // advance() changes the clock but does not guarantee the timer task has finished.
        tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
            loop {
                let state = self.read().await?;
                if state.grant.is_some() {
                    return Ok(state);
                }
                tokio::time::sleep(Duration::from_millis(/*millis*/ 1)).await;
            }
        })
        .await?
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.processor.clear_runtime_references();
        self.drain.abort();
    }
}

#[tokio::test]
async fn control_timeout_grants_after_a_full_minute() -> Result<()> {
    let f = Fixture::new().await?;
    tokio::time::pause();
    let pending = f.request("one").await?;
    assert_eq!(
        pending.request.expect("control request").status,
        ThreadControlRequestStatus::Pending
    );
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 59)).await;
    assert_eq!(f.read().await?.grant, None);
    tokio::time::advance(Duration::from_millis(/*millis*/ 999)).await;
    assert_eq!(f.read().await?.grant, None);
    tokio::time::advance(Duration::from_millis(/*millis*/ 1)).await;
    assert_eq!(
        f.wait_for_grant()
            .await?
            .grant
            .expect("control grant")
            .controller_id,
        "dot"
    );
    Ok(())
}

#[tokio::test]
async fn denied_request_timer_cannot_grant_a_replacement_request_early() -> Result<()> {
    let f = Fixture::new().await?;
    tokio::time::pause();
    f.request("one").await?;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
    f.update(ThreadControlAction::Deny {
        request_id: "one".to_owned(),
    })
    .await?;
    f.request("two").await?;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
    tokio::task::yield_now().await;
    assert_eq!(f.read().await?.grant, None);
    tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
    let state = f.wait_for_grant().await?;
    assert_eq!(state.request.expect("control request").id, "two");
    assert!(state.grant.is_some());
    Ok(())
}

#[tokio::test]
async fn locking_cancels_timeout_and_locked_requests_never_auto_grant() -> Result<()> {
    let f = Fixture::new().await?;
    tokio::time::pause();
    f.request("one").await?;
    f.update(ThreadControlAction::Lock).await?;
    f.request("two").await?;
    tokio::time::advance(Duration::from_secs(/*secs*/ 120)).await;
    tokio::task::yield_now().await;
    let state = f.read().await?;
    assert_eq!(state.grant, None);
    assert_eq!(
        state.request.expect("control request").status,
        ThreadControlRequestStatus::Pending
    );
    f.update(ThreadControlAction::Approve {
        request_id: "two".to_owned(),
    })
    .await?;
    assert!(f.read().await?.locked);
    Ok(())
}

#[tokio::test]
async fn human_submission_revokes_before_waiting_in_the_thread_queue() -> Result<()> {
    use crate::request_serialization::RequestSerializationAccess;
    use crate::request_serialization::RequestSerializationQueueKey;
    let f = Fixture::new().await?;
    f.update(ThreadControlAction::Grant {
        controller_id: "dot".to_owned(),
    })
    .await?;
    let (started, waiting) = tokio::sync::oneshot::channel();
    let (release, gate) = tokio::sync::oneshot::channel();
    f.processor
        .request_serialization_queues
        .enqueue_background(
            RequestSerializationQueueKey::Thread {
                thread_id: f.id.clone(),
            },
            RequestSerializationAccess::Exclusive,
            async move {
                let _ = started.send(());
                let _ = gate.await;
            },
        )
        .await;
    waiting.await?;
    f.processor.process_request(ConnectionId(1), serde_json::from_value(json!({
        "id":3,"method":"turn/start","params":{
            "threadId":f.id,"input":[{"type":"text","text":"My next instruction","text_elements":[]}]
        }
    }))?, &AppServerTransport::WebSocket { bind_address: "127.0.0.1:0".parse()? }, Arc::clone(&f.session)).await;
    assert_eq!(f.read().await?.grant, None);
    let _ = release.send(());
    Ok(())
}
