use crate::AppServerTarget;
use crate::RemoteAppServerEndpoint;
use crate::chatwidget::ConnectedModeFooterState;
use codex_app_server_client::AppServerClient;

#[derive(Clone, Debug, PartialEq, Eq)]
enum RequestedAppServerMode {
    Embedded,
    LocalShared { endpoint: RemoteAppServerEndpoint },
    Remote { endpoint: RemoteAppServerEndpoint },
}

pub(crate) struct ResolvedAppServerMode {
    target: AppServerTarget,
    initial_app_server: Option<AppServerClient>,
    footer_state: Option<ConnectedModeFooterState>,
}

impl ResolvedAppServerMode {
    pub(crate) fn for_unconnected_target(
        target: AppServerTarget,
        footer_state: Option<ConnectedModeFooterState>,
    ) -> Self {
        Self {
            target,
            initial_app_server: None,
            footer_state,
        }
    }

    pub(crate) fn target(&self) -> &AppServerTarget {
        &self.target
    }

    pub(crate) fn footer_state(&self) -> Option<ConnectedModeFooterState> {
        self.footer_state
    }

    pub(crate) fn into_initial_app_server(self) -> Option<AppServerClient> {
        self.initial_app_server
    }
}

fn resolve_requested_app_server_mode(
    cli_local: Option<RemoteAppServerEndpoint>,
    cli_remote: Option<RemoteAppServerEndpoint>,
    configured_local: Option<String>,
) -> color_eyre::Result<RequestedAppServerMode> {
    if let Some(endpoint) = cli_remote {
        return Ok(RequestedAppServerMode::Remote { endpoint });
    }

    if let Some(endpoint) = cli_local {
        return Ok(RequestedAppServerMode::LocalShared { endpoint });
    }

    if let Some(configured_local) = configured_local {
        return Ok(RequestedAppServerMode::LocalShared {
            endpoint: crate::resolve_remote_addr(&configured_local)?,
        });
    }

    Ok(RequestedAppServerMode::Embedded)
}

pub(crate) async fn resolve_app_server_mode(
    cli_local: Option<RemoteAppServerEndpoint>,
    cli_remote: Option<RemoteAppServerEndpoint>,
    configured_local: Option<String>,
) -> color_eyre::Result<ResolvedAppServerMode> {
    match resolve_requested_app_server_mode(cli_local, cli_remote, configured_local)? {
        RequestedAppServerMode::Embedded => Ok(ResolvedAppServerMode {
            target: AppServerTarget::Embedded,
            initial_app_server: None,
            footer_state: None,
        }),
        RequestedAppServerMode::LocalShared { endpoint } => {
            match crate::connect_remote_app_server(endpoint.clone()).await {
                Ok(app_server) => Ok(ResolvedAppServerMode {
                    target: AppServerTarget::LocalDaemon { endpoint },
                    initial_app_server: Some(app_server),
                    footer_state: Some(ConnectedModeFooterState::Connected),
                }),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        endpoint = ?endpoint,
                        "failed to reach local shared app-server; starting private embedded app-server instead"
                    );
                    Ok(ResolvedAppServerMode {
                        target: AppServerTarget::Embedded,
                        initial_app_server: None,
                        footer_state: Some(ConnectedModeFooterState::LocalFallback),
                    })
                }
            }
        }
        RequestedAppServerMode::Remote { endpoint } => {
            let app_server = crate::connect_remote_app_server(endpoint.clone()).await?;
            Ok(ResolvedAppServerMode {
                target: AppServerTarget::Remote { endpoint },
                initial_app_server: Some(app_server),
                footer_state: Some(ConnectedModeFooterState::Connected),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequestedAppServerMode;
    use super::resolve_requested_app_server_mode;
    use crate::RemoteAppServerEndpoint;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn resolve_requested_app_server_mode_uses_configured_local_shared() {
        let mode = resolve_requested_app_server_mode(
            /*cli_local*/ None,
            /*cli_remote*/ None,
            Some("ws://127.0.0.1:4500".to_string()),
        )
        .expect("configured local shared app-server should parse");

        assert_eq!(
            mode,
            RequestedAppServerMode::LocalShared {
                endpoint: RemoteAppServerEndpoint::WebSocket {
                    websocket_url: "ws://127.0.0.1:4500/".to_string(),
                    auth_token: None,
                },
            }
        );
    }

    #[test]
    fn resolve_requested_app_server_mode_prefers_cli_remote_over_local_shared() {
        let mode = resolve_requested_app_server_mode(
            /*cli_local*/ None,
            Some(RemoteAppServerEndpoint::WebSocket {
                websocket_url: "ws://127.0.0.1:4100/".to_string(),
                auth_token: Some("token".to_string()),
            }),
            Some("ws://127.0.0.1:4500".to_string()),
        )
        .expect("cli remote should win");

        assert_eq!(
            mode,
            RequestedAppServerMode::Remote {
                endpoint: RemoteAppServerEndpoint::WebSocket {
                    websocket_url: "ws://127.0.0.1:4100/".to_string(),
                    auth_token: Some("token".to_string()),
                },
            }
        );
    }

    #[test]
    fn resolve_requested_app_server_mode_accepts_cli_local_unix_socket() {
        let socket_path = AbsolutePathBuf::try_from("/tmp/codex.sock")
            .expect("absolute unix socket path should parse");
        let mode = resolve_requested_app_server_mode(
            Some(RemoteAppServerEndpoint::UnixSocket {
                socket_path: socket_path.clone(),
            }),
            /*cli_remote*/ None,
            /*configured_local*/ None,
        )
        .expect("cli local unix socket should parse");

        assert_eq!(
            mode,
            RequestedAppServerMode::LocalShared {
                endpoint: RemoteAppServerEndpoint::UnixSocket { socket_path },
            }
        );
    }
}
