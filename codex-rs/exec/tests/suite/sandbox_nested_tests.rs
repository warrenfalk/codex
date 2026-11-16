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
async fn nested_sandbox_starts_and_preserves_file_restrictions() -> anyhow::Result<()> {
    let Some(sh) = find_executable_on_path("sh") else {
        eprintln!("sh not found, skipping nested sandbox test");
        return Ok(());
    };
    let Some(sandbox_env) = linux_sandbox_test_env().await else {
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let workspace = AbsolutePathBuf::try_from(std::fs::canonicalize(temp.path())?)?;
    let protected = workspace.join("protected");
    std::fs::write(&protected, "original")?;
    let helper = workspace.join("codex-linux-sandbox");
    std::os::unix::fs::symlink(codex_utils_cargo_bin::cargo_bin("codex-exec")?, &helper)?;
    let writable_policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::try_from("/")?.into(),
            FileSystemAccessMode::Read,
        ),
        FileSystemSandboxEntry::new(workspace.clone().into(), FileSystemAccessMode::Write),
        FileSystemSandboxEntry::new(protected.clone().into(), FileSystemAccessMode::Read),
    ]);
    let script = r#"
set -eu
test "$(cat protected)" = original
if (printf changed > protected) 2>/dev/null; then exit 41; fi
printf 'nested sandbox ready\n'
"#;
    for file_system in [FileSystemSandboxPolicy::read_only(), writable_policy] {
        for network in [
            NetworkSandboxPolicy::Restricted,
            NetworkSandboxPolicy::Enabled,
        ] {
            let profile = PermissionProfile::from_runtime_permissions(&file_system, network);
            let output = spawn_command_under_sandbox(
                vec![
                    helper.to_string_lossy().into_owned(),
                    "--sandbox-policy-cwd".into(),
                    workspace.to_string_lossy().into_owned(),
                    "--permission-profile".into(),
                    serde_json::to_string(&profile)?,
                    "--".into(),
                    sh.to_string_lossy().into_owned(),
                    "-c".into(),
                    script.into(),
                ],
                workspace.clone(),
                &profile,
                &workspace,
                StdioPolicy::RedirectForShellTool,
                sandbox_env.clone(),
            )
            .await?
            .wait_with_output()
            .await?;
            assert!(
                output.status.success(),
                "nested sandbox failed for {profile:?}: {output:?}"
            );
            assert_eq!(output.stdout, b"nested sandbox ready\n");
        }
    }
    assert_eq!(std::fs::read_to_string(protected)?, "original");
    Ok(())
}
