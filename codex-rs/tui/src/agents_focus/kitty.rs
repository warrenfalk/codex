//! Run Kitty's acknowledged focus operation from the TUI's own terminal context.

use anyhow::Context;
use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

pub(crate) fn command(env: impl Fn(&'static str) -> Option<OsString>) -> anyhow::Result<Command> {
    anyhow::ensure!(
        env("TMUX").is_none() && env("STY").is_none(),
        "Focusing a TUI inside tmux or screen is unavailable; its inner pane cannot be selected"
    );
    let window_id = env("KITTY_WINDOW_ID")
        .and_then(|value| value.into_string().ok())
        .context("This TUI has no Kitty window ID; focusing requires a TUI running in Kitty")?
        .parse::<u64>()
        .context("This TUI has an invalid KITTY_WINDOW_ID")?;
    anyhow::ensure!(window_id > 0, "This TUI has an invalid KITTY_WINDOW_ID");
    let mut command = Command::new("kitten");
    command.args([
        "@",
        "--use-password",
        "never",
        "focus-window",
        "--match",
        &format!("id:{window_id}"),
    ]);
    Ok(command)
}

pub(crate) async fn focus(mut command: Command) -> anyhow::Result<()> {
    // No password discovery/prompt, notification, or launch fallback. The
    // target TUI owns the controlling TTY and KITTY_LISTEN_ON environment.
    let output = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 2),
        command.stdin(Stdio::null()).kill_on_drop(true).output(),
    )
    .await
    .context("Kitty focus timed out; check that remote control is enabled")?
    .context("Could not run kitten for this TUI")?;
    anyhow::ensure!(
        output.status.success(),
        "Kitty focus failed ({}): {}{}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim(),
        String::from_utf8_lossy(&output.stdout).trim(),
    );
    Ok(())
}
