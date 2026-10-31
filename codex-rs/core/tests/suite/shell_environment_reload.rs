use anyhow::Result;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ProjectEnvMode;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_wine_exec;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commands_reload_configured_environment_between_turns() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_wine_exec!(
        Ok(()),
        "basic PowerShell execution through Wine is unavailable"
    );
    let harness =
        TestCodexHarness::with_auto_env_builder(test_codex().with_model("gpt-5.4")).await?;
    let file = harness.test().config.codex_home.join("config.toml");
    let command = match core_test_support::test_target_os() {
        core_test_support::TestTargetOs::Linux | core_test_support::TestTargetOs::MacOs => {
            "printf 'value=%s' \"${CODEX_TEST_RELOADED_ENV:-unset}\""
        }
        core_test_support::TestTargetOs::Windows => {
            "if ($env:CODEX_TEST_RELOADED_ENV) { Write-Output value=$env:CODEX_TEST_RELOADED_ENV } else { Write-Output value=unset }"
        }
    };
    for (index, value) in [Some("first"), Some("second"), None]
        .into_iter()
        .enumerate()
    {
        let config = value.map_or_else(String::new, |value| {
            format!("[shell_environment_policy.set]\nCODEX_TEST_RELOADED_ENV = '{value}'\n")
        });
        std::fs::write(&file, config)?;
        let call_id = format!("environment-{index}");
        mount_sse_sequence(
            harness.server(),
            vec![
                sse(vec![
                    ev_response_created(&format!("command-{index}")),
                    ev_function_call(
                        &call_id,
                        "exec_command",
                        &json!({"cmd": command, "yield_time_ms": 5000, "login": false}).to_string(),
                    ),
                    ev_completed(&format!("command-{index}")),
                ]),
                sse(vec![
                    ev_assistant_message("done", "done"),
                    ev_completed("done"),
                ]),
            ],
        )
        .await;
        harness
            .submit("print the configured environment variable")
            .await?;
        let stdout = harness.function_call_stdout(&call_id).await;
        assert!(
            stdout.contains(&format!("value={}", value.unwrap_or("unset"))),
            "{stdout}"
        );
    }

    // User shell commands run on the host even when model execution uses a remote executor.
    let host_command = match codex_core::shell::default_user_shell().name() {
        "powershell" => "Write-Output value=$env:CODEX_TEST_RELOADED_ENV",
        "cmd" => "echo value=%CODEX_TEST_RELOADED_ENV%",
        _ => "printf 'value=%s' \"$CODEX_TEST_RELOADED_ENV\"",
    };
    std::fs::write(
        &file,
        "[shell_environment_policy.set]\nCODEX_TEST_RELOADED_ENV = 'user-shell'\n",
    )?;
    harness
        .test()
        .codex
        .submit(Op::RunUserShellCommand {
            command: host_command.to_string(),
            timeout_ms: None,
            project_env: ProjectEnvMode::Bypass,
        })
        .await?;
    let EventMsg::ExecCommandEnd(output) = wait_for_event(&harness.test().codex, |event| {
        matches!(event, EventMsg::ExecCommandEnd(_))
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(
        (output.exit_code, output.aggregated_output.trim()),
        (0, "value=user-shell")
    );
    Ok(())
}
