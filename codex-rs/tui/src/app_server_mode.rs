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
            let target = AppServerTarget::LocalDaemon { endpoint };
            match crate::app_server_connection::connect(&target).await {
                Ok(app_server) => Ok(ResolvedAppServerMode {
                    target,
                    initial_app_server: Some(app_server),
                    footer_state: Some(ConnectedModeFooterState::Connected),
                }),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        target = ?target,
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
            let target = AppServerTarget::Remote { endpoint };
            let app_server = crate::app_server_connection::connect(&target).await?;
            Ok(ResolvedAppServerMode {
                target,
                initial_app_server: Some(app_server),
                footer_state: Some(ConnectedModeFooterState::Connected),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequestedAppServerMode;
    use super::resolve_app_server_mode;
    use super::resolve_requested_app_server_mode;
    use crate::AppServerTarget;
    use crate::RemoteAppServerEndpoint;
    use crate::chatwidget::ConnectedModeFooterState;
    use crate::legacy_core::config::ConfigBuilder;
    use codex_config::LoaderOverrides;
    use codex_exec_server::EnvironmentManager;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn unreachable_local_shared_falls_back_only_at_startup() -> color_eyre::Result<()> {
        let home = TempDir::new()?;
        let endpoint = RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::try_from(home.path().join("missing.sock"))?,
        };
        let mode = resolve_app_server_mode(
            Some(endpoint.clone()),
            /*cli_remote*/ None,
            /*configured_local*/ None,
        )
        .await?;
        assert_eq!(
            (mode.target(), mode.footer_state()),
            (
                &AppServerTarget::Embedded,
                Some(ConnectedModeFooterState::LocalFallback)
            ),
        );
        assert!(mode.into_initial_app_server().is_none());

        let config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .build()
            .await?;
        let picker = crate::start_app_server_for_picker(
            &config,
            &AppServerTarget::LocalDaemon { endpoint },
            /*state_db*/ None,
            Arc::new(EnvironmentManager::default_for_tests()),
        )
        .await;
        assert!(
            picker.is_err(),
            "a disconnected shared picker must not start an embedded server"
        );
        Ok(())
    }

    #[tokio::test]
    async fn unreachable_remote_never_falls_back() -> color_eyre::Result<()> {
        let home = TempDir::new()?;
        let endpoint = RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::try_from(home.path().join("missing.sock"))?,
        };
        assert!(
            resolve_app_server_mode(
                /*cli_local*/ None,
                Some(endpoint),
                /*configured_local*/ None,
            )
            .await
            .is_err()
        );
        Ok(())
    }

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
