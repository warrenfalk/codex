use super::*;
#[cfg(unix)]
use pretty_assertions::assert_eq;

#[cfg(unix)]
#[tokio::test]
async fn desktop_handler_receives_original_uri_and_success_is_quiet() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir()?;
    let received = directory.path().join("received");
    let browser_called = directory.path().join("browser-called");
    let handler_name = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let handler = directory.path().join(handler_name);
    std::fs::write(&handler, "#!/bin/sh\nprintf '%s' \"$1\" > \"$RECEIVED\"\n")?;
    std::fs::set_permissions(&handler, std::fs::Permissions::from_mode(/*mode*/ 0o755))?;
    let browser = directory.path().join("browser");
    std::fs::write(&browser, "#!/bin/sh\nprintf called > \"$BROWSER_CALLED\"\n")?;
    std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(/*mode*/ 0o755))?;

    for url in [
        "vscode://file/tmp/a%20b.tsx:132:4",
        "cursor://file/tmp/it's $(literal);&.rs:12",
        "https://example.com/path?q=a%20b&other=1#section",
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut command = desktop_command(url);
        command.env("PATH", directory.path());
        command.env("RECEIVED", &received);
        command.env("BROWSER", &browser);
        command.env("BROWSER_CALLED", &browser_called);

        open_with_command(url.to_string(), command, AppEventSender::new(sender)).await;

        assert_eq!(std::fs::read_to_string(&received)?, url);
        assert!(!browser_called.exists());
        assert!(receiver.recv().await.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn unsuccessful_handler_reports_error_in_transcript() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    #[cfg(unix)]
    let mut command = Command::new("sh");
    #[cfg(unix)]
    command.args(["-c", "exit 3"]);
    #[cfg(windows)]
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    command.args(["-NoProfile", "-NonInteractive", "-Command", "exit 3"]);

    open_with_command(
        "vscode://file/tmp/example.rs:132".to_string(),
        command,
        AppEventSender::new(sender),
    )
    .await;

    let Some(AppEvent::InsertHistoryCell(cell)) = receiver.recv().await else {
        panic!("failed desktop handoff must report an error");
    };
    let rendered = cell
        .display_lines(/*width*/ 80)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("desktop_link_launch_failed", rendered);
    assert!(receiver.recv().await.is_none());
}

#[tokio::test]
async fn missing_handler_reports_launch_failure() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let command = Command::new(directory.path().join("missing-handler"));
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    open_with_command(
        "vscode://file/tmp/example.rs:132".to_string(),
        command,
        AppEventSender::new(sender),
    )
    .await;

    let Some(AppEvent::InsertHistoryCell(cell)) = receiver.recv().await else {
        panic!("missing desktop handler must report an error");
    };
    let rendered = cell
        .display_lines(/*width*/ 200)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered
            .contains("Failed to open vscode://file/tmp/example.rs:132 with the desktop handler:")
    );
    assert!(receiver.recv().await.is_none());
    Ok(())
}
