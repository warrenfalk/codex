use std::env;
use std::io;

use codex_protocol::ThreadId;
use tokio::process::Command;

const THREAD_VAR_NAME: &str = "codex_thread_id";

fn inside_kitty() -> bool {
    env::var_os("KITTY_WINDOW_ID").is_some()
}

fn match_spec(thread_id: &str) -> String {
    format!("var:{THREAD_VAR_NAME}={thread_id}")
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

pub(crate) async fn focus_thread(thread_id: &str) -> io::Result<()> {
    if !inside_kitty() {
        return Err(io::Error::other(
            "local session focusing currently requires running inside kitty",
        ));
    }

    let status = Command::new("kitten")
        .args(["@", "focus-window", "--match", &match_spec(thread_id)])
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "failed to focus kitty window for thread {thread_id}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn match_spec_uses_thread_var_name() {
        assert_eq!(
            match_spec("123e4567-e89b-12d3-a456-426614174000"),
            "var:codex_thread_id=123e4567-e89b-12d3-a456-426614174000"
        );
    }
}
