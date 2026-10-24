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
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

#[tokio::test]
async fn config_protection_allows_external_git_writes_without_synthetic_locks() -> anyhow::Result<()>
{
    let Some(sh) = find_executable_on_path("sh") else {
        eprintln!("sh not found, skipping Git config sandbox test");
        return Ok(());
    };
    let Some(git) = find_executable_on_path("git") else {
        eprintln!("git not found, skipping Git config sandbox test");
        return Ok(());
    };
    let Some(sandbox_env) = linux_sandbox_test_env().await else {
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let workspace = AbsolutePathBuf::try_from(std::fs::canonicalize(temp.path())?)?;
    let git_dir = workspace.join(".git");
    std::fs::create_dir(&git_dir)?;
    let mut entries = vec![
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::try_from("/")?.into(),
            FileSystemAccessMode::Read,
        ),
        FileSystemSandboxEntry::new(workspace.clone().into(), FileSystemAccessMode::Write),
        FileSystemSandboxEntry::new(git_dir.clone().into(), FileSystemAccessMode::Write),
    ];
    for name in ["config", "config.worktree"] {
        let path = git_dir.join(name);
        std::fs::write(&path, "[codex]\n\tprobe = original\n")?;
        entries.push(FileSystemSandboxEntry::new(
            path.into(),
            FileSystemAccessMode::Read,
        ));
    }
    let profile = PermissionProfile::from_runtime_permissions(
        &FileSystemSandboxPolicy::restricted(entries),
        NetworkSandboxPolicy::Restricted,
    );
    let script = r#"
set -eu
for config in .git/config .git/config.worktree; do
    test ! -e "$config.lock"
    if printf changed >> "$config"; then exit 41; fi
    if "$1" config --file "$config" codex.probe inside; then exit 42; fi
    test ! -e "$config.lock"
done
printf 'ready\n'
while ! test -e release; do sleep 0.01; done
"#;
    let mut child = spawn_command_under_sandbox(
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
    .await?;
    let mut stdout = BufReader::new(child.stdout.take().expect("sandbox stdout"));
    let mut ready = String::new();
    tokio::time::timeout(Duration::from_secs(10), stdout.read_line(&mut ready)).await??;
    assert_eq!(ready, "ready\n");
    for name in ["config", "config.worktree"] {
        let path = git_dir.join(name);
        assert_eq!(
            std::fs::read_to_string(&path)?,
            "[codex]\n\tprobe = original\n"
        );
        assert!(!git_dir.join(format!("{name}.lock")).exists());
        let output = tokio::process::Command::new(&git)
            .args(["config", "--file"])
            .arg(path.as_path())
            .args(["codex.probe", "outside"])
            .output()
            .await?;
        assert!(
            output.status.success(),
            "outside Git config failed: {output:?}"
        );
        assert_eq!(
            std::fs::read_to_string(path)?,
            "[codex]\n\tprobe = outside\n"
        );
    }
    std::fs::write(workspace.join("release"), "")?;
    let output = child.wait_with_output().await?;
    assert!(output.status.success(), "sandbox failed: {output:?}");
    Ok(())
}
