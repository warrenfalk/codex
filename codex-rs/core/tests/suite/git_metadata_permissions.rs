use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use codex_file_system::WriteFileOptions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::TurnEnvironmentRequests;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
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
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

const GIT_CONFIG: &str =
    "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\tlogallrefupdates = true\n";
const GIT_WORKSPACE_CONFIG: &str = r#"approval_policy = "never"
default_permissions = "git-workspace"

[permissions.git-workspace]
extends = ":workspace"

[permissions.git-workspace.filesystem]
":git_metadata" = "write"
"#;

async fn create_directory(
    fs: &dyn ExecutorFileSystem,
    cwd: &AbsolutePathBuf,
    relative_path: &str,
) -> Result<()> {
    fs.create_directory(
        &PathUri::from_abs_path(&cwd.join(relative_path)),
        CreateDirectoryOptions {
            recursive: true,
            follow_symlinks: true,
        },
        /*sandbox*/ None,
    )
    .await?;
    Ok(())
}

async fn write_file(
    fs: &dyn ExecutorFileSystem,
    cwd: &AbsolutePathBuf,
    relative_path: &str,
    contents: &str,
) -> Result<()> {
    fs.write_file(
        &PathUri::from_abs_path(&cwd.join(relative_path)),
        contents.as_bytes().to_vec(),
        WriteFileOptions::default(),
        /*sandbox*/ None,
    )
    .await?;
    Ok(())
}

async fn init_git_fixture(fs: &dyn ExecutorFileSystem, cwd: &AbsolutePathBuf) -> Result<()> {
    for path in [
        ".git/objects/info",
        ".git/objects/pack",
        ".git/refs/heads",
        ".git/refs/tags",
        ".git/hooks",
        ".git/info",
        ".git/branches",
    ] {
        create_directory(fs, cwd, path).await?;
    }
    write_file(fs, cwd, ".git/HEAD", "ref: refs/heads/main\n").await?;
    write_file(fs, cwd, ".git/config", GIT_CONFIG).await?;
    write_file(fs, cwd, "tracked.txt", "initial\n").await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opted_in_profile_can_commit_and_manage_worktrees_but_cannot_change_git_controls()
-> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_target_windows!(
        Ok(()),
        "the remote Windows test image does not provide a native Git executable"
    );

    let home = Arc::new(TempDir::new()?);
    fs::write(home.path().join("config.toml"), GIT_WORKSPACE_CONFIG)?;

    let builder = test_codex()
        .with_home(home)
        .with_model("gpt-5.4")
        .with_workspace_setup(|cwd, fs| async move { init_git_fixture(fs.as_ref(), &cwd).await });
    let harness = TestCodexHarness::with_auto_env_builder(builder).await?;

    let call_id = "git-commit";
    let second_call_id = "git-commit-second-step";
    let command = concat!(
        "set -eu; ",
        "test ! -e .git/config.lock && test ! -e .git/config.worktree.lock || exit 40; ",
        "git add tracked.txt; ",
        "git -c user.name='Codex Test' -c user.email='codex@example.com' ",
        "commit --quiet --no-gpg-sign --allow-empty -m checkpoint; ",
        "git worktree add --quiet --detach comparison HEAD; ",
        "printf 'comparison\\n' >> comparison/tracked.txt; ",
        "git -C comparison add tracked.txt; ",
        "git -C comparison -c user.name='Codex Test' -c user.email='codex@example.com' ",
        "commit --quiet --no-gpg-sign -m comparison; ",
        "git worktree remove comparison; ",
        "git worktree add --quiet --detach stale-comparison HEAD; ",
        "rm -r stale-comparison; ",
        "git worktree prune --expire now; ",
        "test ! -e .git/worktrees/stale-comparison; ",
        "if git config codex.must-not-persist true; then exit 41; fi; ",
        "if printf '#!/bin/sh\\n' > .git/hooks/pre-commit; then exit 42; fi; ",
        "test ! -e .git/hooks/pre-commit; ",
        "git rev-parse --verify HEAD"
    );
    let args = serde_json::to_string(&json!({
        "cmd": command,
        "yield_time_ms": 30_000,
        "login": false,
    }))?;
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "exec_command", &args),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_function_call(second_call_id, "exec_command", &args),
                ev_completed("resp-2"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-3"),
            ]),
        ],
    )
    .await;

    harness
        .test()
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "commit the file and manage comparison worktrees".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(&harness.test().codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output = harness.function_call_stdout(call_id).await;
    assert!(
        output.contains("Process exited with code 0"),
        "unexpected output: {output}"
    );
    let second_output = harness.function_call_stdout(second_call_id).await;
    assert!(
        second_output.contains("Process exited with code 0"),
        "unexpected output after capturing the next step: {second_output}"
    );
    assert_eq!(harness.read_file_text(".git/config").await?, GIT_CONFIG);
    assert!(!harness.path_exists(".git/config.lock").await?);
    assert!(!harness.path_exists(".git/config.worktree.lock").await?);
    assert!(!harness.path_exists(".git/hooks/pre-commit").await?);
    assert!(!harness.path_exists("comparison").await?);
    assert!(!harness.path_exists("stale-comparison").await?);
    assert!(!harness.path_exists(".git/worktrees/comparison").await?);
    assert!(
        !harness
            .path_exists(".git/worktrees/stale-comparison")
            .await?
    );
    assert!(harness.path_exists(".git/refs/heads/main").await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opted_in_profile_can_stage_in_an_attached_project_with_git_controls_protected()
-> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_target_windows!(
        Ok(()),
        "the remote Windows test image does not provide a native Git executable"
    );

    let home = Arc::new(TempDir::new()?);
    fs::write(home.path().join("config.toml"), GIT_WORKSPACE_CONFIG)?;
    let builder = test_codex()
        .with_home(home)
        .with_model("gpt-5.4")
        .with_workspace_setup(|cwd, fs| async move {
            init_git_fixture(fs.as_ref(), &cwd).await?;
            init_git_fixture(fs.as_ref(), &cwd.join("attached")).await
        });
    let harness = TestCodexHarness::with_auto_env_builder(builder).await?;
    let test = harness.test();
    let mut environment = test.executor_environment().request();
    environment.workspace_roots = vec![environment.cwd.clone(), environment.cwd.join("attached")?];

    let call_id = "stage-in-both-projects";
    let command = concat!(
        "set -eu; ",
        "git add tracked.txt; ",
        "git -C attached add tracked.txt; ",
        "if git -C attached config codex.must-not-persist true; then exit 41; fi; ",
        "if printf '#!/bin/sh\\n' > attached/.git/hooks/pre-commit; then exit 42; fi; ",
        "git -C attached ls-files --error-unmatch tracked.txt"
    );
    let args = serde_json::to_string(&json!({
        "cmd": command,
        "yield_time_ms": 30_000,
        "login": false,
    }))?;
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "exec_command", &args),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let cwd = test.config.cwd.clone();
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "stage the file in both projects".to_string(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                runtime_workspace_roots: Some(vec![cwd.clone(), cwd.join("attached")]),
                environments: Some(TurnEnvironmentRequests::new(cwd, vec![environment])),
                ..Default::default()
            }),
        )
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output = harness.function_call_stdout(call_id).await;
    assert!(
        output.contains("Process exited with code 0"),
        "unexpected output: {output}"
    );
    assert!(harness.path_exists(".git/index").await?);
    assert!(harness.path_exists("attached/.git/index").await?);
    assert_eq!(
        harness.read_file_text("attached/.git/config").await?,
        GIT_CONFIG
    );
    assert!(
        !harness
            .path_exists("attached/.git/hooks/pre-commit")
            .await?
    );
    Ok(())
}
