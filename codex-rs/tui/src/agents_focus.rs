//! Focus an existing local TUI through its live app state, without attaching to the server.

use codex_app_server_client::RemoteAppServerEndpoint;

#[cfg(unix)]
mod service;
#[cfg(unix)]
pub(crate) use service::Action;
#[cfg(unix)]
pub(crate) use service::Endpoint;
#[cfg(unix)]
pub(crate) use service::FocusService;
#[cfg(unix)]
pub(crate) use service::PendingRequest;
#[cfg(unix)]
pub(crate) use service::Reply;
#[cfg(unix)]
pub(crate) use service::registry_directory;
#[cfg(unix)]
pub(crate) mod kitty;

/// Focus a local TUI currently displaying the requested session on this endpoint.
pub async fn focus_agent_tui(
    endpoint: RemoteAppServerEndpoint,
    session_id: &str,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    return service::focus(&service::registry_directory(), endpoint, session_id).await;
    #[cfg(not(unix))]
    {
        let _ = (endpoint, session_id);
        anyhow::bail!("Focusing an existing Codex TUI is only supported in Kitty on Unix")
    }
}
