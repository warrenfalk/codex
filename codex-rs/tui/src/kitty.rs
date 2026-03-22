use std::env;
use std::io;

use tokio::process::Command;

fn inside_kitty() -> bool {
    env::var_os("KITTY_WINDOW_ID").is_some()
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
