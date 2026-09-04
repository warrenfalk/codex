use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use codex_file_system::WriteFileOptions;
use codex_protocol::protocol::EventMsg;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opted_in_profile_can_commit_but_cannot_change_git_controls() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_target_windows!(
        Ok(()),
        "the remote Windows test image does not provide a native Git executable"
    );

    let home = Arc::new(TempDir::new()?);
    fs::write(
        home.path().join("config.toml"),
        r#"approval_policy = "never"
default_permissions = "git-workspace"

[permissions.git-workspace]
extends = ":workspace"

[permissions.git-workspace.filesystem]
":git_metadata" = "write"
"#,
    )?;

    let builder = test_codex()
        .with_home(home)
        .with_model("gpt-5.4")
        .with_workspace_setup(|cwd, fs| async move {
            for path in [
                ".git/objects/info",
                ".git/objects/pack",
                ".git/refs/heads",
                ".git/refs/tags",
                ".git/hooks",
                ".git/info",
                ".git/branches",
            ] {
                create_directory(fs.as_ref(), &cwd, path).await?;
            }
            write_file(fs.as_ref(), &cwd, ".git/HEAD", "ref: refs/heads/main\n").await?;
            write_file(fs.as_ref(), &cwd, ".git/config", GIT_CONFIG).await?;
            write_file(fs.as_ref(), &cwd, "tracked.txt", "initial\n").await?;
            Ok(())
        });
    let harness = TestCodexHarness::with_auto_env_builder(builder).await?;

    let call_id = "git-commit";
    let command = concat!(
        "git add tracked.txt && ",
        "git -c user.name='Codex Test' -c user.email='codex@example.com' ",
        "commit --quiet --no-gpg-sign -m initial && ",
        "if git config codex.must-not-persist true; then exit 41; fi; ",
        "if printf '#!/bin/sh\\n' > .git/hooks/pre-commit; then exit 42; fi; ",
        "test ! -e .git/hooks/pre-commit && ",
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
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    harness
        .test()
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "stage and commit the file".to_string(),
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
    assert_eq!(harness.read_file_text(".git/config").await?, GIT_CONFIG);
    assert!(!harness.path_exists(".git/hooks/pre-commit").await?);
    assert!(harness.path_exists(".git/refs/heads/main").await?);
    Ok(())
}
