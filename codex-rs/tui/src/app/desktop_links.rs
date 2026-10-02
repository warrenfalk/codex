//! Hand transcript links to the desktop's registered URL handler without blocking input.
//!
//! Browser-specific launchers bypass editor protocol associations. Keep the original URI intact
//! and let the desktop select its application; only failed handoffs belong in the transcript.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell;
use std::process::Stdio;
use tokio::process::Command;

pub(super) fn open(url: String, events: AppEventSender) {
    let command = desktop_command(&url);
    tokio::spawn(open_with_command(url, command, events));
}

fn desktop_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference = 'Stop'; Start-Process -FilePath $env:CODEX_DESKTOP_LINK_URI",
        ]);
        // Pass the URI as data, never as PowerShell source or a command-shell argument.
        command.env("CODEX_DESKTOP_LINK_URI", url);
        command
    };

    #[cfg(not(any(target_os = "macos", windows)))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command
}

async fn open_with_command(url: String, mut command: Command, events: AppEventSender) {
    let error = match command.status().await {
        Ok(status) if status.success() => return,
        Ok(status) => match status.code() {
            Some(code) => format!("URL handler exited with code {code}"),
            None => format!("URL handler terminated with {status}"),
        },
        Err(error) => error.to_string(),
    };
    events.send(AppEvent::InsertHistoryCell(Box::new(
        history_cell::new_error_event(format!(
            "Failed to open {url} with the desktop handler: {error}"
        )),
    )));
}

#[cfg(test)]
#[path = "desktop_links_tests.rs"]
mod tests;
