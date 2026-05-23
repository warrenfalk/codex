//! Connected-mode reconnect handling for the TUI app.
//!
//! This module owns the remote app-server reconnection loop and the state transitions needed to
//! reattach the current thread or start a fresh unsaved session after the transport drops.

use super::*;
use crate::app_event::ConnectedBackendReconnect;

impl App {
    pub(super) fn stop_connected_reconnect_task(&mut self) {
        if let Some(task) = self.connected_reconnect_task.take() {
            task.abort();
        }
    }

    pub(super) fn handle_connected_backend_disconnected(
        &mut self,
        app_server: &AppServerSession,
        message: String,
    ) {
        if self.connected_backend_disconnected {
            return;
        }

        self.connected_backend_disconnected = true;
        self.app_server_footer_state =
            Some(crate::chatwidget::ConnectedModeFooterState::Disconnected);
        self.chat_widget.show_disconnected_mode_footer();
        self.chat_widget.add_error_message(message);
        self.chat_widget.show_connected_backend_reconnect_status();

        let Some(endpoint) = (match &self.app_server_target {
            crate::AppServerTarget::Embedded => None,
            crate::AppServerTarget::LocalDaemon { endpoint }
            | crate::AppServerTarget::Remote { endpoint } => Some(endpoint.clone()),
        }) else {
            self.app_event_tx.send(AppEvent::FatalExitRequest(
                "connected mode is missing its app-server endpoint".to_string(),
            ));
            return;
        };
        let Some(thread_id) = self.primary_thread_id else {
            self.app_event_tx.send(AppEvent::FatalExitRequest(
                "connected mode lost track of its primary thread id".to_string(),
            ));
            return;
        };
        let config = self.config.clone();
        let app_event_tx = self.app_event_tx.clone();
        let thread_params_mode = app_server.thread_params_mode();
        let remote_cwd_override = app_server.remote_cwd_override().map(Path::to_path_buf);
        let app_server_url_is_loopback = match &endpoint {
            crate::RemoteAppServerEndpoint::WebSocket { websocket_url, .. } => {
                url::Url::parse(websocket_url)
                    .ok()
                    .and_then(|url| {
                        url.host().map(|host| match host {
                            url::Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
                            url::Host::Ipv4(addr) => addr.is_loopback(),
                            url::Host::Ipv6(addr) => addr.is_loopback(),
                        })
                    })
                    .unwrap_or(false)
            }
            crate::RemoteAppServerEndpoint::UnixSocket { .. } => true,
        };
        let start_fresh_after_reconnect =
            self.primary_session_configured
                .as_ref()
                .is_none_or(|session| {
                    session.rollout_path.as_deref().is_none_or(|path| {
                        app_server_url_is_loopback && !rollout_path_is_resumable(path)
                    })
                });

        self.stop_connected_reconnect_task();
        self.connected_reconnect_task = Some(tokio::spawn(async move {
            loop {
                match crate::connect_remote_app_server(endpoint.clone()).await {
                    Ok(client) => {
                        let mut app_server = AppServerSession::new(client, thread_params_mode)
                            .with_remote_cwd_override(remote_cwd_override.clone());
                        let started_result = if start_fresh_after_reconnect {
                            tracing::warn!(
                                %thread_id,
                                "connected session has no materialized rollout; reconnect will start a fresh thread"
                            );
                            app_server
                                .start_thread_with_session_start_source(
                                    &config, /*session_start_source*/ None,
                                )
                                .await
                        } else {
                            app_server.resume_thread(config.clone(), thread_id).await
                        };
                        match started_result {
                            Ok(started) => {
                                app_event_tx.send(AppEvent::ConnectedBackendReconnected(Box::new(
                                    ConnectedBackendReconnect {
                                        app_server,
                                        started,
                                    },
                                )));
                                break;
                            }
                            Err(err) => {
                                if start_fresh_after_reconnect {
                                    tracing::warn!(
                                        error = %err,
                                        %thread_id,
                                        "failed to start fresh connected session after reconnect"
                                    );
                                } else {
                                    tracing::warn!(
                                        error = %err,
                                        %thread_id,
                                        "failed to resume connected session after reconnect"
                                    );
                                    let no_rollout_message =
                                        format!("no rollout found for thread id {thread_id}");
                                    let server_reports_missing_rollout =
                                        err.chain().any(|cause| {
                                            cause
                                                .downcast_ref::<codex_app_server_client::TypedRequestError>()
                                                .is_some_and(|typed| match typed {
                                                    codex_app_server_client::TypedRequestError::Server {
                                                        source,
                                                        ..
                                                    } => {
                                                        source.message == no_rollout_message
                                                            || source.message.contains("is not materialized yet")
                                                    }
                                                    codex_app_server_client::TypedRequestError::Transport {
                                                        ..
                                                    }
                                                    | codex_app_server_client::TypedRequestError::Deserialize {
                                                        ..
                                                    } => false,
                                                })
                                        });
                                    if server_reports_missing_rollout {
                                        tracing::warn!(
                                            %thread_id,
                                            "connected session has no materialized rollout; reconnect will start a fresh thread"
                                        );
                                        match app_server
                                            .start_thread_with_session_start_source(
                                                &config, /*session_start_source*/ None,
                                            )
                                            .await
                                        {
                                            Ok(started) => {
                                                app_event_tx.send(
                                                    AppEvent::ConnectedBackendReconnected(
                                                        Box::new(ConnectedBackendReconnect {
                                                            app_server,
                                                            started,
                                                        }),
                                                    ),
                                                );
                                                break;
                                            }
                                            Err(err) => {
                                                tracing::warn!(
                                                    error = %err,
                                                    %thread_id,
                                                    "failed to start fresh connected session after reconnect"
                                                );
                                            }
                                        }
                                    }
                                }
                                if let Err(shutdown_err) = app_server.shutdown().await {
                                    tracing::warn!(
                                        error = %shutdown_err,
                                        "failed to shut down failed reconnect app-server"
                                    );
                                }
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            %thread_id,
                            "failed to reconnect remote app server"
                        );
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }));
    }

    pub(super) async fn handle_connected_backend_reconnected(
        &mut self,
        app_server: &mut AppServerSession,
        reconnect: ConnectedBackendReconnect,
    ) -> Result<()> {
        self.connected_backend_disconnected = false;
        self.app_server_footer_state = Some(crate::chatwidget::ConnectedModeFooterState::Connected);
        self.stop_connected_reconnect_task();

        let previous = std::mem::replace(app_server, reconnect.app_server);
        if let Err(err) = previous.shutdown().await {
            tracing::warn!(error = %err, "failed to shut down disconnected app server");
        }

        let AppServerStartedThread { session, turns } = reconnect.started;
        let restored_same_thread = self.primary_thread_id == Some(session.thread_id);
        self.chat_widget.show_connected_mode_footer();
        self.chat_widget.clear_connected_backend_reconnect_status();
        if restored_same_thread {
            self.primary_session_configured = Some(session.clone());
            let channel = self.ensure_thread_channel(session.thread_id);
            {
                let mut store = channel.store.lock().await;
                store.set_session(session.clone(), turns);
                store.rebase_buffer_after_session_refresh();
            }

            if self.active_thread_id == Some(session.thread_id) {
                self.chat_widget.suppress_next_session_info_cell();
                self.chat_widget.handle_thread_session(session);
            }
            self.chat_widget
                .add_info_message("Reconnected to app-server.".to_string(), None);
        } else {
            self.clear_active_thread().await;
            self.enqueue_primary_thread_session(session, turns).await?;
            self.chat_widget.add_info_message(
                "Connected to a new app-server session because the previous session was not yet saved."
                    .to_string(),
                None,
            );
        }
        self.backfill_loaded_subagent_threads(app_server).await;
        Ok(())
    }
}
