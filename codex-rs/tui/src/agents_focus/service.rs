//! Private per-user rendezvous for local TUIs. Requests are checked by the app at
//! dispatch time; the socket never stores a potentially stale session association.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use anyhow::Context;
use codex_app_server_client::RemoteAppServerEndpoint;
use serde::Deserialize;
use serde::Serialize;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 4);
const MAX_MESSAGE_BYTES: u64 = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Endpoint {
    Unix(PathBuf),
    WebSocket(String),
}

impl From<&RemoteAppServerEndpoint> for Endpoint {
    fn from(endpoint: &RemoteAppServerEndpoint) -> Self {
        match endpoint {
            RemoteAppServerEndpoint::UnixSocket { socket_path } => Self::Unix(
                std::fs::canonicalize(socket_path).unwrap_or_else(|_| socket_path.to_path_buf()),
            ),
            RemoteAppServerEndpoint::WebSocket { websocket_url, .. } => {
                Self::WebSocket(websocket_url.clone())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Action {
    Probe,
    Focus,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Request {
    pub(crate) endpoint: Endpoint,
    pub(crate) session_id: String,
    pub(crate) action: Action,
}

#[derive(Debug)]
pub(crate) struct PendingRequest {
    pub(crate) request: Request,
    pub(crate) response: oneshot::Sender<Reply>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Reply {
    NoMatch,
    Ready,
    Focused,
    Unavailable(String),
}

pub(crate) fn registry_directory() -> PathBuf {
    // A short, stable path shared by GUI launchers and TUIs, irrespective of
    // CODEX_HOME, cwd, TMPDIR, or whether the process inherited XDG_RUNTIME_DIR.
    PathBuf::from(format!("/tmp/codex-tui-{}", unsafe { libc::geteuid() }))
}

fn validate_directory(directory: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(directory)?;
    anyhow::ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0,
        "TUI focus directory must be a private directory owned by this user: {}",
        directory.display()
    );
    Ok(())
}

pub(crate) struct FocusService {
    task: JoinHandle<()>,
    socket: PathBuf,
}

impl FocusService {
    pub(crate) fn start(directory: &Path, events: AppEventSender) -> anyhow::Result<Self> {
        match std::fs::DirBuilder::new().mode(0o700).create(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        validate_directory(directory)?;
        let socket = directory.join(format!(
            "{:010}-{}.sock",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let listener = UnixListener::bind(&socket)?;
        let task = tokio::spawn(async move {
            // Serve one bounded request at a time. Dropping the service also
            // cancels its in-flight request and closes the app's response channel.
            while let Ok((stream, _)) = listener.accept().await {
                let _ = tokio::time::timeout(REQUEST_TIMEOUT, serve(stream, &events)).await;
            }
        });
        Ok(Self { task, socket })
    }
}

impl Drop for FocusService {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.socket);
    }
}

async fn serve(stream: UnixStream, events: &AppEventSender) -> anyhow::Result<()> {
    let mut stream = BufReader::new(stream);
    let request = read_message(&mut stream).await?;
    let (response, receiver) = oneshot::channel();
    events.send(AppEvent::AgentsFocusRequested(PendingRequest {
        request,
        response,
    }));
    let mut closed = [0];
    let reply = tokio::select! {
        reply = receiver => reply?,
        // The client must keep its write side open until it gets the reply.
        _ = stream.read(&mut closed) => return Ok(()),
    };
    let mut bytes = serde_json::to_vec(&reply)?;
    bytes.push(b'\n');
    stream.get_mut().write_all(&bytes).await?;
    Ok(())
}

async fn read_message<T: serde::de::DeserializeOwned>(
    stream: &mut BufReader<UnixStream>,
) -> anyhow::Result<T> {
    let mut bytes = Vec::new();
    stream
        .take(MAX_MESSAGE_BYTES)
        .read_until(b'\n', &mut bytes)
        .await?;
    anyhow::ensure!(
        bytes.last() == Some(&b'\n'),
        "Incomplete or oversized TUI focus message"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

async fn request(socket: &Path, request: &Request) -> anyhow::Result<Reply> {
    tokio::time::timeout(REQUEST_TIMEOUT + Duration::from_secs(/*secs*/ 1), async {
        let mut stream = UnixStream::connect(socket).await?;
        let mut bytes = serde_json::to_vec(request)?;
        bytes.push(b'\n');
        stream.write_all(&bytes).await?;
        read_message(&mut BufReader::new(stream)).await
    })
    .await
    .context("TUI did not respond to the focus request")?
}

pub(super) async fn focus(
    directory: &Path,
    endpoint: RemoteAppServerEndpoint,
    session_id: &str,
) -> anyhow::Result<()> {
    let no_match = || {
        anyhow::anyhow!(
            "No running local TUI is attached to session {session_id} on the selected server (TUIs must support --focus)"
        )
    };
    match validate_directory(directory) {
        Ok(()) => {}
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Err(no_match());
        }
        Err(error) => return Err(error),
    }
    let mut sockets = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if file_type.is_socket() && entry.path().extension().is_some_and(|ext| ext == "sock") {
            sockets.push(entry.path());
        }
    }
    // Zero-padded PIDs give a deterministic choice among live, focusable TUIs.
    sockets.sort();
    let mut unavailable = None;
    let mut query = Request {
        endpoint: Endpoint::from(&endpoint),
        session_id: session_id.into(),
        action: Action::Probe,
    };
    for socket in sockets {
        match request(&socket, &query).await {
            Ok(Reply::Ready) => {
                query.action = Action::Focus;
                // Recheck in the app, since a session switch may have happened
                // after the probe. Never retry an ambiguous focus failure.
                match request(&socket, &query)
                    .await
                    .context("TUI became unavailable before focus could be confirmed")?
                {
                    Reply::Focused => return Ok(()),
                    Reply::NoMatch => query.action = Action::Probe,
                    Reply::Unavailable(message) => anyhow::bail!("Could not focus TUI: {message}"),
                    Reply::Ready => anyhow::bail!("TUI did not acknowledge focusing the session"),
                }
            }
            Ok(Reply::NoMatch) => {}
            Ok(Reply::Unavailable(message)) => {
                unavailable.get_or_insert(message);
            }
            Ok(Reply::Focused) => anyhow::bail!("Unexpected TUI focus response"),
            Err(error)
                if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    )
                }) => {}
            Err(error) => {
                unavailable.get_or_insert(format!("TUI {}: {error:#}", socket.display()));
            }
        }
    }
    match unavailable {
        Some(message) => anyhow::bail!("TUI focus unavailable: {message}"),
        None => Err(no_match()),
    }
}
