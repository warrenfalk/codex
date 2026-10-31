use anyhow::Result;
use codex_config::types::ApprovalsReviewer;
use codex_core::TurnInputRequest;
use codex_core::config::Constrained;
use codex_features::Feature;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_target_windows;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;

#[test_case("deny", ReviewDecision::Approved; "deny approved")]
#[test_case("none", ReviewDecision::Approved; "none approved")]
#[test_case("deny", ReviewDecision::denied("fixture access declined"); "deny declined")]
#[test_case("none", ReviewDecision::denied("fixture access declined"); "none declined")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denied_reads_follow_command_approval(
    access: &str,
    decision: ReviewDecision,
) -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_target_windows!(
        Ok(()),
        "the Windows test executor cannot enforce denied filesystem reads with its restricted-token sandbox"
    );

    let access: FileSystemAccessMode = serde_json::from_value(json!(access))?;
    let harness =
        TestCodexHarness::with_auto_env_builder(test_codex().with_config(move |config| {
            let mut file_system = FileSystemSandboxPolicy::default();
            file_system.entries.push(FileSystemSandboxEntry::new(
                FileSystemPath::GlobPattern {
                    pattern: format!("{}/**/*.secret", config.cwd.as_path().display()),
                },
                access,
            ));
            config
                .permissions
                .set_permission_profile(PermissionProfile::from_runtime_permissions(
                    &file_system,
                    NetworkSandboxPolicy::Restricted,
                ))
                .expect("set denied read profile");
            config.permissions.approval_policy =
                Constrained::allow_any(AskForApproval::TrustSandbox);
            config.approvals_reviewer = ApprovalsReviewer::User;
            config
                .features
                .enable(Feature::WriteStdinApproval)
                .expect("enable approval for terminal input");
            config
                .features
                .disable(Feature::ShellZshFork)
                .expect("disable intercepted shell execution");
        }))
        .await?;
    let secret = "approved-command-can-read-this-fixture";
    harness.write_file("blocked.secret", secret).await?;

    let mut responses = Vec::new();
    for (call_id, sandbox_permissions) in [
        ("before", "use_default"),
        ("escalated", "require_escalated"),
        ("after", "use_default"),
    ] {
        let args = json!({
            "cmd": if call_id == "escalated" {
                "read -r line; cat blocked.secret"
            } else {
                "cat blocked.secret 2>/dev/null || printf blocked"
            },
            "login": false,
            "tty": call_id == "escalated",
            "yield_time_ms": if call_id == "escalated" { 100 } else { 10_000 },
            "sandbox_permissions": sandbox_permissions,
        });
        responses.push(sse(vec![
            ev_response_created(call_id),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed(call_id),
        ]));
        if call_id == "escalated" && decision == ReviewDecision::Approved {
            responses.push(sse(vec![
                ev_response_created("input"),
                ev_function_call(
                    "input",
                    "write_stdin",
                    &json!({"session_id":1001, "chars":"continue\n", "yield_time_ms":10_000})
                        .to_string(),
                ),
                ev_completed("input"),
            ]));
        }
    }
    responses.push(sse(vec![
        ev_assistant_message("done", "done"),
        ev_completed("complete"),
    ]));
    let mock = mount_sse_sequence(harness.server(), responses).await;
    let codex = &harness.test().codex;
    codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "read the fixture before, during, and after escalation".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;

    let mut approved_calls = Vec::new();
    loop {
        let event = wait_for_event(codex, |event| {
            matches!(
                event,
                EventMsg::ExecApprovalRequest(_) | EventMsg::TurnComplete(_)
            )
        })
        .await;
        match event {
            EventMsg::ExecApprovalRequest(approval) => {
                approved_calls.push(approval.effective_approval_id());
                codex
                    .submit(Op::ExecApproval {
                        id: approval.effective_approval_id(),
                        turn_id: Some(approval.turn_id),
                        decision: decision.clone(),
                    })
                    .await?;
            }
            EventMsg::TurnComplete(_) => break,
            event => panic!("unexpected event: {event:?}"),
        }
    }
    let expected_approvals = if decision == ReviewDecision::Approved {
        vec!["escalated", "input"]
    } else {
        vec!["escalated"]
    };
    assert_eq!(approved_calls, expected_approvals);
    for call_id in ["before", "after"] {
        let output = mock
            .function_call_output_text(call_id)
            .expect("command output");
        assert!(
            output.trim_end().ends_with("blocked"),
            "{call_id}: {output}"
        );
    }
    let output_id = if decision == ReviewDecision::Approved {
        "input"
    } else {
        "escalated"
    };
    let output = mock
        .function_call_output_text(output_id)
        .expect("escalation output");
    assert_eq!(
        output.contains(secret),
        decision == ReviewDecision::Approved,
        "{output}"
    );
    Ok(())
}
