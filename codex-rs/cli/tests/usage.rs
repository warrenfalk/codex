use anyhow::Result;
use codex_rollout::parse_rollout_line;
use codex_utils_absolute_path::test_support::test_path_buf;
use serde_json::json;
use tempfile::TempDir;

#[test]
fn usage_table_renders_rollout_costs() -> Result<()> {
    let home = TempDir::new()?;
    let project = test_path_buf("/usage-project");
    let sessions = home.path().join("sessions/2000/01/01");
    std::fs::create_dir_all(&sessions)?;
    let thread_id = "00000000-0000-0000-0000-000000000001";
    let timestamp = "2000-01-01T12:00:00Z";
    let usage = json!({
        "input_tokens": 120,
        "cached_input_tokens": 20,
        "output_tokens": 30,
        "reasoning_output_tokens": 10,
        "total_tokens": 150,
    });
    let lines = [
        json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "timestamp": timestamp,
                "cwd": project,
                "originator": "test",
                "cli_version": "test",
                "source": "cli",
                "model_provider": "openai",
            },
        }),
        json!({
            "timestamp": timestamp,
            "type": "turn_context",
            "payload": {
                "cwd": project,
                "approval_policy": "never",
                "sandbox_policy": {"type": "read-only"},
                "model": "gpt-5.4",
            },
        }),
        json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {"total_token_usage": usage, "last_token_usage": usage},
            },
        }),
    ];
    let contents = lines
        .into_iter()
        .map(|line| {
            parse_rollout_line(&line.to_string()).and_then(|line| serde_json::to_string(&line))
        })
        .collect::<serde_json::Result<Vec<_>>>()?
        .join("\n");
    std::fs::write(
        sessions.join(format!("rollout-2000-01-01T12-00-00-{thread_id}.jsonl")),
        contents,
    )?;

    let output = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", home.path())
        .args(["usage", "--since", "2000-01-01", "--until", "2000-01-02"])
        .assert()
        .success()
        .stderr(predicates::str::is_empty())
        .get_output()
        .stdout
        .clone();
    let rendered =
        String::from_utf8(output)?.replace(project.to_string_lossy().as_ref(), "[PROJECT]");
    insta::assert_snapshot!("usage_report_table", rendered);
    Ok(())
}
