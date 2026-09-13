use std::path::Path;

use anyhow::Result;
use app_test_support::app_server_json_shutdown_event;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[test]
fn strict_config_rejects_unknown_config_fields_for_app_server() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        r#"
foo = "bar"
"#,
    )?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["app-server", "--strict-config", "--listen", "off"])
        .assert()
        .failure()
        .stderr(contains("unknown configuration field"));

    Ok(())
}

#[test]
fn agents_accept_interactive_configuration_overrides() -> Result<()> {
    let codex_home = TempDir::new()?;

    for args in [
        ["-c", "features.multi_agent_mode=true", "agents"].as_slice(),
        ["--enable", "multi_agent_mode", "agents"].as_slice(),
        ["--yolo", "agents"].as_slice(),
        ["--search", "agents"].as_slice(),
        ["--model", "gpt-5", "agents"].as_slice(),
        ["--approve-for-me", "agents"].as_slice(),
        ["--cd", ".", "agents"].as_slice(),
    ] {
        let mut cmd = codex_command(codex_home.path())?;
        cmd.env("TERM", "xterm-256color").args(args);
        #[cfg(not(any(unix, windows)))]
        cmd.args(["--remote", "ws://127.0.0.1:4512"]);

        cmd.assert()
            .failure()
            .stderr(contains("stdin is not a terminal"));
    }

    Ok(())
}

#[test]
fn agents_reject_inputs_that_cannot_be_applied() -> Result<()> {
    let codex_home = TempDir::new()?;

    for (args, expected_error) in [
        (
            ["--image=image.png", "agents"].as_slice(),
            "does not accept an initial prompt or images",
        ),
        (
            ["--oss", "agents", "--remote", "ws://127.0.0.1:4512"].as_slice(),
            "cannot apply local provider or additional-directory overrides",
        ),
        (
            [
                "--add-dir",
                ".",
                "agents",
                "--remote",
                "ws://127.0.0.1:4512",
            ]
            .as_slice(),
            "cannot apply local provider or additional-directory overrides",
        ),
        (
            [
                "-c",
                "sandbox_workspace_write.writable_roots=[\"../shared\"]",
                "agents",
                "--remote",
                "ws://127.0.0.1:4512",
            ]
            .as_slice(),
            "cannot apply local provider or additional-directory overrides",
        ),
    ] {
        let mut cmd = codex_command(codex_home.path())?;
        cmd.args(args)
            .assert()
            .failure()
            .stderr(contains(expected_error));
    }

    Ok(())
}

#[test]
fn agents_json_uses_only_observer_requests_and_needs_no_terminal() -> Result<()> {
    use tokio_tungstenite::tungstenite;
    let codex_home = TempDir::new()?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("ws://{}", listener.local_addr()?);
    let server = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        let mut websocket = tungstenite::accept(socket).unwrap();
        let mut subscribed = false;
        while let Ok(tungstenite::Message::Text(text)) = websocket.read() {
            let request: serde_json::Value = serde_json::from_str(&text).unwrap();
            let result = match request["method"].as_str().unwrap() {
                "initialize" => json!({"userAgent": "mock"}),
                "initialized" => continue,
                "event/firehose" => {
                    subscribed = true;
                    json!({})
                }
                "thread/list" | "thread/loaded/list" => {
                    assert!(subscribed);
                    assert_eq!(request["params"]["cwd"], serde_json::Value::Null);
                    json!({"data": [], "nextCursor": null})
                }
                method => panic!("unexpected observer request: {method}"),
            };
            websocket
                .send(tungstenite::Message::Text(
                    json!({"id": request["id"], "result": result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
    });
    let output = codex_command(codex_home.path())?
        .current_dir(codex_home.path())
        .env("TERM", "dumb")
        .args(["agents", "--json", "--remote", &endpoint])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout)?,
        json!({
            "version": 1, "connection": "connected", "counts": {"total": 0, "working": 0, "needsAttention": 0}, "sessions": []
        })
    );
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(output.stderr, Vec::<u8>::new());
    server.join().unwrap();
    Ok(())
}

#[test]
fn agents_json_does_not_start_a_missing_daemon() -> Result<()> {
    let codex_home = TempDir::new()?;
    codex_command(codex_home.path())?
        .current_dir(codex_home.path())
        .args(["agents", "--json"])
        .assert()
        .failure()
        .stdout("");
    assert!(!codex_home.path().join("sessions").exists());
    Ok(())
}

#[test]
fn app_server_emits_json_info_events() -> Result<()> {
    let codex_home = TempDir::new()?;
    let event = app_server_json_shutdown_event("codex", &["app-server"], codex_home.path())?;

    assert_eq!(
        event,
        json!({
            "level": "INFO",
            "fields": {
                "message": "processor task exited",
                "exit_reason": "stdio_connection_closed",
                "remaining_connection_count": 0,
                "shutdown_forced": false,
            },
            "target": "codex_app_server",
        })
    );

    Ok(())
}
