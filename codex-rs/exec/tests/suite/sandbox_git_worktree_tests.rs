use super::find_executable_on_path;
use super::linux_sandbox_test_env;
use super::spawn_command_under_sandbox;
use codex_core::spawn::StdioPolicy;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn git_worktree_management_preserves_config_and_hook_protection() -> anyhow::Result<()> {
    let Some(sh) = find_executable_on_path("sh") else {
        eprintln!("sh not found, skipping Git worktree sandbox test");
        return Ok(());
    };
    let Some(git) = find_executable_on_path("git") else {
        eprintln!("git not found, skipping Git worktree sandbox test");
        return Ok(());
    };
    let Some(sandbox_env) = linux_sandbox_test_env().await else {
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let workspace = AbsolutePathBuf::try_from(std::fs::canonicalize(temp.path())?)?;
    let output = tokio::process::Command::new(&git)
        .args(["init", "--quiet", "--initial-branch=main", "--template="])
        .arg(workspace.as_path())
        .output()
        .await?;
    assert!(output.status.success(), "git init failed: {output:?}");
    let git_dir = workspace.join(".git");
    std::fs::create_dir(git_dir.join("hooks"))?;
    std::fs::write(workspace.join("tracked.txt"), "initial\n")?;
    let original_config = std::fs::read_to_string(git_dir.join("config"))?;
    let mut entries = vec![
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::try_from("/")?.into(),
            FileSystemAccessMode::Read,
        ),
        FileSystemSandboxEntry::new(workspace.clone().into(), FileSystemAccessMode::Write),
        FileSystemSandboxEntry::new(git_dir.clone().into(), FileSystemAccessMode::Write),
    ];
    for name in ["config", "hooks"] {
        entries.push(FileSystemSandboxEntry::new(
            git_dir.join(name).into(),
            FileSystemAccessMode::Read,
        ));
    }
    let profile = PermissionProfile::from_runtime_permissions(
        &FileSystemSandboxPolicy::restricted(entries),
        NetworkSandboxPolicy::Restricted,
    );
    let script = r#"
set -eu
git="$1"
test ! -e .git/worktrees
"$git" add tracked.txt
"$git" -c user.name='Codex Test' -c user.email='codex@example.com' commit --quiet --no-gpg-sign -m initial
"$git" worktree add --quiet --detach comparison HEAD
printf 'comparison\n' >> comparison/tracked.txt
"$git" -C comparison add tracked.txt
"$git" -C comparison -c user.name='Codex Test' -c user.email='codex@example.com' commit --quiet --no-gpg-sign -m comparison
"$git" worktree remove comparison
"$git" worktree add --quiet --detach stale-comparison HEAD
rm -r stale-comparison
"$git" worktree prune --expire now
if "$git" config codex.must-not-persist true; then exit 41; fi
if printf '#!/bin/sh\n' > .git/hooks/pre-commit; then exit 42; fi
printf 'worktrees managed\n'
"#;
    let output = spawn_command_under_sandbox(
        vec![
            sh.to_string_lossy().into_owned(),
            "-c".into(),
            script.into(),
            "sh".into(),
            git.to_string_lossy().into_owned(),
        ],
        workspace.clone(),
        &profile,
        &workspace,
        StdioPolicy::RedirectForShellTool,
        sandbox_env,
    )
    .await?
    .wait_with_output()
    .await?;
    assert!(output.status.success(), "sandbox failed: {output:?}");
    assert_eq!(output.stdout, b"worktrees managed\n");
    assert_eq!(
        std::fs::read_to_string(git_dir.join("config"))?,
        original_config
    );
    assert!(!git_dir.join("hooks/pre-commit").exists());
    for name in ["comparison", "stale-comparison"] {
        assert!(!workspace.join(name).exists());
        assert!(!git_dir.join("worktrees").join(name).exists());
    }
    Ok(())
}
