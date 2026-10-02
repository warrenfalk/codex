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
use codex_utils_absolute_path::test_support::PathBufExt;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn writable_device_grants_preserve_other_sandbox_restrictions() -> anyhow::Result<()> {
    let Some(sh) = find_executable_on_path("sh") else {
        eprintln!("sh not found in PATH, skipping test.");
        return Ok(());
    };
    let Some(sandbox_env) = linux_sandbox_test_env().await else {
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let workspace = tokio::fs::canonicalize(temp.path()).await?.abs();
    tokio::fs::write(workspace.join("read-only.txt"), "read-only fixture").await?;
    tokio::fs::write(workspace.join("blocked.secret"), "denied fixture").await?;
    let device_link = workspace.join("device-link");
    std::os::unix::fs::symlink("/dev/null", &device_link)?;
    let device_directory_link = workspace.join("device-directory-link");
    std::os::unix::fs::symlink("/dev", &device_directory_link)?;

    // Standard devices keep this independent of GPU hardware. Rebinding them
    // with an ordinary writable bind makes opening the device fail with EACCES.
    for device_grant in [
        AbsolutePathBuf::try_from("/dev/null")?,
        AbsolutePathBuf::try_from("/dev")?,
        device_link,
        device_directory_link,
    ] {
        let file_system_policy = FileSystemSandboxPolicy::restricted(vec![
            FileSystemSandboxEntry::new(
                AbsolutePathBuf::try_from("/")?.into(),
                FileSystemAccessMode::Read,
            ),
            FileSystemSandboxEntry::new(workspace.clone().into(), FileSystemAccessMode::Write),
            FileSystemSandboxEntry::new(device_grant.clone().into(), FileSystemAccessMode::Write),
            FileSystemSandboxEntry::new(
                AbsolutePathBuf::try_from("/dev/zero")?.into(),
                FileSystemAccessMode::Deny,
            ),
            FileSystemSandboxEntry::new(
                AbsolutePathBuf::try_from("/dev/full")?.into(),
                FileSystemAccessMode::Read,
            ),
            FileSystemSandboxEntry::new(
                workspace.join("read-only.txt").into(),
                FileSystemAccessMode::Read,
            ),
            FileSystemSandboxEntry::new(
                workspace.join("blocked.secret").into(),
                FileSystemAccessMode::Deny,
            ),
        ]);
        let permission_profile = PermissionProfile::from_runtime_permissions(
            &file_system_policy,
            NetworkSandboxPolicy::Restricted,
        );
        let mut script = r#"exec 3<> /dev/null || exit 10
if (exec 4< /dev/zero); then exit 11; fi
if (exec 4> read-only.txt); then exit 12; fi
if (exec 4< blocked.secret); then exit 13; fi
printf allowed > writable.txt
"#
        .to_string();
        if device_grant.as_path().is_dir() {
            script.push_str("if (exec 4<> /dev/full); then exit 14; fi\n");
        }
        let child = spawn_command_under_sandbox(
            vec![sh.to_string_lossy().into_owned(), "-c".to_string(), script],
            workspace.clone(),
            &permission_profile,
            &workspace,
            StdioPolicy::RedirectForShellTool,
            sandbox_env.clone(),
        )
        .await?;
        let output = child.wait_with_output().await?;
        assert!(
            output.status.success(),
            "device grant {}: {output:?}",
            device_grant.display(),
        );
    }

    assert_eq!(
        (
            tokio::fs::read_to_string(workspace.join("read-only.txt")).await?,
            tokio::fs::read_to_string(workspace.join("blocked.secret")).await?,
            tokio::fs::read_to_string(workspace.join("writable.txt")).await?,
        ),
        (
            "read-only fixture".to_string(),
            "denied fixture".to_string(),
            "allowed".to_string(),
        ),
    );
    Ok(())
}
