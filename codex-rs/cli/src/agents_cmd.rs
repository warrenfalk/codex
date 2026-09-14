use super::*;

#[derive(Debug, Parser)]
pub(crate) struct AgentsCommand {
    /// Print a complete agents dashboard snapshot as one JSONL line.
    #[arg(long)]
    pub(crate) json: bool,

    /// Keep emitting snapshots as the dashboard changes (requires --json).
    #[arg(long, requires = "json")]
    pub(crate) watch: bool,

    /// Focus an existing local TUI displaying this session, without opening one.
    #[arg(long, value_name = "SESSION_ID", conflicts_with_all = ["json", "watch"])]
    pub(crate) focus: Option<String>,

    #[clap(flatten)]
    pub(crate) remote: InteractiveRemoteOptions,

    /// Use this directory for new tasks on a remote server.
    #[arg(long = "cd", short = 'C', value_name = "DIR")]
    pub(crate) cwd: Option<PathBuf>,

    /// Disable alternate screen mode.
    #[arg(long = "no-alt-screen", default_value_t = false)]
    pub(crate) no_alt_screen: bool,
}

pub(crate) enum AgentsAction {
    Json { watch: bool },
    Focus { session_id: String },
}

pub(crate) async fn run_noninteractive(
    interactive: &TuiCli,
    local: Option<String>,
    remote: Option<String>,
    remote_auth_token_env: Option<String>,
    action: AgentsAction,
) -> anyhow::Result<()> {
    let remote_endpoint = resolve_remote_endpoint(remote, remote_auth_token_env)?;
    let remote_workspace = remote_endpoint.is_some();
    let codex_home = find_codex_home()?;
    let config_cwd = match interactive.cwd.as_ref().filter(|_| !remote_workspace) {
        Some(cwd) => AbsolutePathBuf::relative_to_current_dir(cwd)?,
        None => AbsolutePathBuf::current_dir()?,
    };
    let config = load_config_toml_with_layer_stack(
        &codex_home,
        Some(&config_cwd),
        interactive
            .config_overrides
            .parse_overrides()
            .map_err(anyhow::Error::msg)?,
        ConfigLoadOptions {
            strict_config: interactive.strict_config,
            loader_overrides: loader_overrides_for_profile_at_codex_home(
                interactive.config_profile_v2.as_ref(),
                &codex_home,
            ),
            ..Default::default()
        },
    )
    .await?
    .config_toml;
    let local = local.or_else(|| {
        config
            .tui
            .as_ref()
            .and_then(|tui| tui.local_app_server_url.clone())
    });
    let implicit_local_daemon = remote_endpoint.is_none() && local.is_none();
    let endpoint = match remote_endpoint {
        Some(endpoint) => endpoint,
        None => codex_tui::resolve_remote_addr(local.as_deref().unwrap_or("unix://"))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
    };
    let worktree_grouping = !remote_workspace
        && config
            .features
            .as_ref()
            .and_then(|features| features.entries().get("worktrees").copied())
            .unwrap_or_else(|| codex_features::Feature::Worktrees.default_enabled());
    match action {
        AgentsAction::Json { watch } => {
            codex_tui::run_agents_json(codex_tui::AgentsJsonOptions {
                endpoint,
                watch,
                worktree_grouping,
                implicit_local_daemon,
            })
            .await
        }
        AgentsAction::Focus { session_id } => {
            codex_tui::focus_agent_tui(endpoint, &session_id).await
        }
    }
}

#[cfg(test)]
#[path = "agents_cmd_tests.rs"]
mod tests;
