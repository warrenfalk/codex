use std::env;
use std::io;

use codex_protocol::ThreadId;
use tokio::process::Command;

const THREAD_VAR_NAME: &str = "codex_thread_id";

fn inside_kitty() -> bool {
    env::var_os("KITTY_WINDOW_ID").is_some()
}

pub(crate) async fn set_current_window_thread_id(thread_id: ThreadId) {
    if !inside_kitty() {
        return;
    }

    let assignment = format!("{THREAD_VAR_NAME}={thread_id}");
    match Command::new("kitten")
        .args(["@", "set-user-vars", &assignment])
        .status()
        .await
    {
        Ok(status) if status.success() => {}
        Ok(status) => {
            tracing::warn!(
                status = status.code(),
                thread_id = %thread_id,
                "failed to set kitty window thread id"
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, thread_id = %thread_id, "failed to run kitten");
        }
    }
}

pub(crate) async fn focus_current_window() -> io::Result<()> {
    if !inside_kitty() {
        return Err(io::Error::other(
            "local session focusing currently requires running inside kitty",
        ));
    }

    let output = Command::new("kitten")
        .args(["@", "focus-window"])
        .output()
        .await?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            "failed to focus current kitty window".to_string()
        } else {
            stderr
        };
        Err(io::Error::other(message))
    }
}
