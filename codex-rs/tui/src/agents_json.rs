//! Passive, complete JSONL snapshots of the agents dashboard.

mod snapshot;
mod state;
#[cfg(test)]
#[path = "agents_json_tests.rs"]
mod tests;

use codex_app_server_client::AppServerClient;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::Thread;
use std::collections::BTreeMap;
use std::io;
use std::io::Write;
use std::time::Duration;

/// Connection and display settings for passive agents output.
pub struct AgentsJsonOptions {
    pub endpoint: RemoteAppServerEndpoint,
    pub watch: bool,
    pub worktree_grouping: bool,
    pub implicit_local_daemon: bool,
}

/// Prints snapshots without entering terminal mode or starting any sessions.
pub async fn run_agents_json(options: AgentsJsonOptions) -> anyhow::Result<()> {
    let mut stdout = io::stdout();
    tokio::select! {
        result = run(&options, &mut stdout) => result,
        result = tokio::signal::ctrl_c() => { result?; Ok(()) }
    }
}

#[allow(clippy::print_stderr)] // JSON owns stdout; connection diagnostics belong on stderr.
async fn run(options: &AgentsJsonOptions, output: &mut impl Write) -> anyhow::Result<()> {
    let mut previous = String::new();
    let mut threads = BTreeMap::<String, Thread>::new();
    let mut backoff = Duration::from_millis(500);
    loop {
        let result = async {
            let args = RemoteAppServerConnectArgs {
                endpoint: options.endpoint.clone(),
                client_name: "codex-agents-json".into(),
                client_version: env!("CARGO_PKG_VERSION").into(),
                experimental_api: true,
                mcp_server_openai_form_elicitation: false,
                opt_out_notification_methods: Vec::new(),
                channel_capacity: 64,
            };
            #[cfg(windows)]
            let client = if options.implicit_local_daemon {
                RemoteAppServerClient::connect_local_daemon(args).await?
            } else {
                RemoteAppServerClient::connect(args).await?
            };
            #[cfg(not(windows))]
            let client = RemoteAppServerClient::connect(args).await?;
            let mut observer = state::Observer {
                client: AppServerClient::Remote(client),
                dirty: Default::default(),
                removed: Default::default(),
                membership: Default::default(),
            };
            let result = async {
                observer.seed(&threads).await?;
                loop {
                    observer.synchronize(&mut threads).await?;
                    emit(
                        output,
                        &mut previous,
                        snapshot::snapshot(&threads, options)?,
                    )?;
                    backoff = Duration::from_millis(500);
                    if !options.watch {
                        return Ok::<_, anyhow::Error>(());
                    }
                    observer.receive().await?;
                }
            }
            .await;
            let _ = observer.client.shutdown().await;
            result
        }
        .await;
        match result {
            Ok(()) => return Ok(()),
            Err(error) if error.downcast_ref::<OutputError>().is_some() => {
                if error
                    .downcast_ref::<OutputError>()
                    .is_some_and(|error| error.0.kind() == io::ErrorKind::BrokenPipe)
                {
                    return Ok(());
                }
                return Err(error);
            }
            Err(error) if !options.watch || !retryable(&error) => return Err(error),
            Err(error) => {
                eprintln!(
                    "agents: {error}; reconnecting in {:.1}s",
                    backoff.as_secs_f64()
                );
                emit(
                    output,
                    &mut previous,
                    serde_json::json!({
                        "version": 1, "connection": "disconnected", "counts": null, "sessions": null
                    }),
                )?;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("writing agents JSON: {0}")]
struct OutputError(#[source] io::Error);

fn emit(
    output: &mut impl Write,
    previous: &mut String,
    snapshot: serde_json::Value,
) -> anyhow::Result<()> {
    let mut line = serde_json::to_string(&snapshot)?;
    line.push('\n');
    if *previous != line {
        output.write_all(line.as_bytes()).map_err(OutputError)?;
        output.flush().map_err(OutputError)?;
        *previous = line;
    }
    Ok(())
}

fn retryable(error: &anyhow::Error) -> bool {
    if let Some(error) = error.downcast_ref::<TypedRequestError>() {
        return match error {
            TypedRequestError::Server { source, .. } => matches!(source.code, -32603 | -32001),
            TypedRequestError::Deserialize { .. } => false,
            TypedRequestError::Transport { source, .. } => transient_io_error(source),
        };
    }
    error
        .downcast_ref::<io::Error>()
        .is_some_and(transient_io_error)
}

fn transient_io_error(error: &io::Error) -> bool {
    !matches!(
        error.kind(),
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData | io::ErrorKind::PermissionDenied
    )
}
