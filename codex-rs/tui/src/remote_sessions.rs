use std::collections::VecDeque;

use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadActivateParams;
use codex_app_server_protocol::ThreadActivateResponse;
use codex_app_server_protocol::ThreadActivateStatus;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use futures::SinkExt;
use futures::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;

pub(crate) struct RemoteSessionsClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_request_id: i64,
    queued_notifications: VecDeque<ServerNotification>,
}

impl RemoteSessionsClient {
    pub(crate) async fn connect(url: &str) -> Result<Self> {
        let (mut ws, _) = connect_async(url)
            .await
            .wrap_err_with(|| format!("failed to connect to app-server websocket at {url}"))?;
        initialize_connection(&mut ws).await?;
        Ok(Self {
            ws,
            next_request_id: 2,
            queued_notifications: VecDeque::new(),
        })
    }

    pub(crate) async fn loaded_thread_ids(&mut self) -> Result<Vec<String>> {
        let mut cursor = None;
        let mut thread_ids = Vec::new();
        loop {
            let response: ThreadLoadedListResponse = self
                .request_typed(
                    "thread/loaded/list",
                    ThreadLoadedListParams {
                        cursor,
                        limit: Some(100),
                    },
                )
                .await?;
            thread_ids.extend(response.data);
            if let Some(next_cursor) = response.next_cursor {
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }
        Ok(thread_ids)
    }

    pub(crate) async fn read_thread(&mut self, thread_id: &str) -> Result<Thread> {
        let response: ThreadReadResponse = self
            .request_typed(
                "thread/read",
                ThreadReadParams {
                    thread_id: thread_id.to_string(),
                    include_turns: true,
                },
            )
            .await?;
        Ok(response.thread)
    }

    pub(crate) async fn activate_thread(&mut self, thread_id: &str) -> Result<()> {
        let response: ThreadActivateResponse = self
            .request_typed(
                "thread/activate",
                ThreadActivateParams {
                    thread_id: thread_id.to_string(),
                },
            )
            .await?;
        match response.status {
            ThreadActivateStatus::Delivered => Ok(()),
            ThreadActivateStatus::NotLoaded => {
                color_eyre::eyre::bail!("thread {thread_id} is not loaded")
            }
            ThreadActivateStatus::NoSubscribers => {
                color_eyre::eyre::bail!("thread {thread_id} has no subscribed clients")
            }
        }
    }

    pub(crate) async fn next_notification(&mut self) -> Result<Option<ServerNotification>> {
        if let Some(notification) = self.queued_notifications.pop_front() {
            return Ok(Some(notification));
        }

        loop {
            let Some(message) = self.read_message().await? else {
                return Ok(None);
            };
            match message {
                JSONRPCMessage::Notification(notification) => {
                    if let Ok(server_notification) = ServerNotification::try_from(notification) {
                        return Ok(Some(server_notification));
                    }
                }
                JSONRPCMessage::Request(request) => {
                    self.reject_request(request.id).await?;
                }
                JSONRPCMessage::Response(_) | JSONRPCMessage::Error(_) => {}
            }
        }
    }

    async fn request_typed<T, P>(&mut self, method: &str, params: P) -> Result<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let request_id = RequestId::Integer(self.next_request_id);
        self.next_request_id += 1;
        self.send_message(JSONRPCMessage::Request(JSONRPCRequest {
            id: request_id.clone(),
            method: method.to_string(),
            params: Some(serde_json::to_value(params)?),
            trace: None,
        }))
        .await?;

        loop {
            let Some(message) = self.read_message().await? else {
                color_eyre::eyre::bail!("app-server websocket connection closed");
            };
            match message {
                JSONRPCMessage::Response(JSONRPCResponse { id, result }) if id == request_id => {
                    let response = serde_json::from_value(result)
                        .wrap_err_with(|| format!("decode `{method}` response"))?;
                    return Ok(response);
                }
                JSONRPCMessage::Error(JSONRPCError { error, id }) if id == request_id => {
                    color_eyre::eyre::bail!("app-server `{method}` failed: {}", error.message);
                }
                JSONRPCMessage::Notification(notification) => {
                    if let Ok(server_notification) = ServerNotification::try_from(notification) {
                        self.queued_notifications.push_back(server_notification);
                    }
                }
                JSONRPCMessage::Request(request) => {
                    self.reject_request(request.id).await?;
                }
                JSONRPCMessage::Response(_) | JSONRPCMessage::Error(_) => {}
            }
        }
    }

    async fn reject_request(&mut self, id: RequestId) -> Result<()> {
        self.send_message(JSONRPCMessage::Error(JSONRPCError {
            id,
            error: JSONRPCErrorError {
                code: -32601,
                message: "sessions picker does not handle app-server requests".to_string(),
                data: None,
            },
        }))
        .await
    }

    async fn read_message(&mut self) -> Result<Option<JSONRPCMessage>> {
        loop {
            let frame = match self.ws.next().await {
                Some(frame) => frame.wrap_err("failed to read websocket frame")?,
                None => return Ok(None),
            };
            match frame {
                WebSocketMessage::Text(text) => {
                    let message = serde_json::from_str(text.as_ref())
                        .wrap_err("failed to decode app-server message")?;
                    return Ok(Some(message));
                }
                WebSocketMessage::Binary(_) => {}
                WebSocketMessage::Ping(_)
                | WebSocketMessage::Pong(_)
                | WebSocketMessage::Frame(_) => {}
                WebSocketMessage::Close(_) => return Ok(None),
            }
        }
    }

    async fn send_message(&mut self, message: JSONRPCMessage) -> Result<()> {
        let payload = serde_json::to_string(&message)?;
        self.ws
            .send(WebSocketMessage::Text(payload.into()))
            .await
            .wrap_err("failed to send websocket frame")
    }
}

async fn initialize_connection(ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>) -> Result<()> {
    let request_id = RequestId::Integer(1);
    let params = InitializeParams {
        client_info: ClientInfo {
            name: "codex-sessions".to_string(),
            title: Some("Codex Sessions".to_string()),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        capabilities: Some(InitializeCapabilities {
            experimental_api: true,
            opt_out_notification_methods: None,
        }),
    };
    let payload = serde_json::to_string(&JSONRPCMessage::Request(JSONRPCRequest {
        id: request_id.clone(),
        method: "initialize".to_string(),
        params: Some(serde_json::to_value(params)?),
        trace: None,
    }))?;
    ws.send(WebSocketMessage::Text(payload.into()))
        .await
        .wrap_err("failed to send websocket frame")?;

    loop {
        let frame = match ws.next().await {
            Some(frame) => frame.wrap_err("failed to read websocket frame")?,
            None => color_eyre::eyre::bail!("websocket stream ended unexpectedly"),
        };
        match frame {
            WebSocketMessage::Text(text) => {
                let message = serde_json::from_str::<JSONRPCMessage>(text.as_ref())
                    .wrap_err("failed to decode app-server message")?;
                match message {
                    JSONRPCMessage::Response(response) if response.id == request_id => break,
                    JSONRPCMessage::Error(JSONRPCError { error, id }) if id == request_id => {
                        color_eyre::eyre::bail!(
                            "app-server `initialize` failed: {}",
                            error.message
                        );
                    }
                    _ => {}
                }
            }
            WebSocketMessage::Binary(_)
            | WebSocketMessage::Ping(_)
            | WebSocketMessage::Pong(_)
            | WebSocketMessage::Frame(_) => {}
            WebSocketMessage::Close(_) => {
                color_eyre::eyre::bail!("websocket stream ended unexpectedly");
            }
        }
    }

    let initialized_payload =
        serde_json::to_string(&JSONRPCMessage::Notification(JSONRPCNotification {
            method: "initialized".to_string(),
            params: None,
        }))?;
    ws.send(WebSocketMessage::Text(initialized_payload.into()))
        .await
        .wrap_err("failed to send websocket frame")
}
