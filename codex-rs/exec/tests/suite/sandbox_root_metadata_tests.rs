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
async fn full_access_with_explicit_restrictions_starts_without_root_metadata_mounts()
-> anyhow::Result<()> {
    let Some(sh) = find_executable_on_path("sh") else {
        eprintln!("sh not found, skipping root metadata sandbox test");
        return Ok(());
    };
    let Some(sandbox_env) = linux_sandbox_test_env().await else {
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let workspace = AbsolutePathBuf::from_absolute_path(std::fs::canonicalize(temp.path())?)?;
    std::fs::write(workspace.join("read-only"), "original")?;
    std::fs::write(workspace.join("denied"), "private")?;
    let policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::try_from("/")?.into(),
            FileSystemAccessMode::Write,
        ),
        FileSystemSandboxEntry::new(
            workspace.join("read-only").into(),
            FileSystemAccessMode::Read,
        ),
        FileSystemSandboxEntry::new(workspace.join("denied").into(), FileSystemAccessMode::Deny),
    ]);
    let profile =
        PermissionProfile::from_runtime_permissions(&policy, NetworkSandboxPolicy::Restricted);
    let script = r#"
set -eu
test "$(cat read-only)" = original
if (printf changed > read-only) 2>/dev/null; then exit 11; fi
if cat denied >/dev/null 2>&1; then exit 12; fi
if (printf changed > denied) 2>/dev/null; then exit 13; fi
printf allowed > writable
"#;
    let child = spawn_command_under_sandbox(
        vec![
            sh.to_string_lossy().into_owned(),
            "-c".into(),
            script.into(),
        ],
        workspace.clone(),
        &profile,
        &workspace,
        StdioPolicy::RedirectForShellTool,
        sandbox_env,
    )
    .await?;
    let output = child.wait_with_output().await?;
    assert!(output.status.success(), "sandbox failed: {output:?}");
    assert_eq!(
        ["read-only", "denied", "writable"]
            .map(|name| std::fs::read_to_string(workspace.join(name)).unwrap()),
        ["original", "private", "allowed"],
    );
    Ok(())
}
