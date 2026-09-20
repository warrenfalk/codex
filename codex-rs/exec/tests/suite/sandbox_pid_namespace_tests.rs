use super::find_executable_on_path;
use super::linux_sandbox_test_env;
use super::spawn_command_under_sandbox;
use codex_core::spawn::StdioPolicy;
use codex_protocol::PidNamespace;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::os::fd::AsRawFd;

#[tokio::test]
async fn pid_namespace_controls_parent_visibility_and_preserves_filesystem_denials()
-> anyhow::Result<()> {
    let Some(python) = find_executable_on_path("python3") else {
        eprintln!("python3 not found, skipping PID namespace test");
        return Ok(());
    };
    let Some(sandbox_env) = linux_sandbox_test_env().await else {
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let workspace = AbsolutePathBuf::try_from(std::fs::canonicalize(temp.path())?)?;
    let denied = workspace.join("denied");
    let read_only = workspace.join("read-only");
    std::fs::write(&denied, "denied fixture")?;
    std::fs::write(&read_only, "read-only fixture")?;
    let parent_file = std::fs::File::open(&denied)?;
    let pid = std::process::id();
    let parent_stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let file_system = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::try_from("/")?.into(),
            FileSystemAccessMode::Read,
        ),
        FileSystemSandboxEntry::new(workspace.clone().into(), FileSystemAccessMode::Write),
        FileSystemSandboxEntry::new(denied.clone().into(), FileSystemAccessMode::Deny),
        FileSystemSandboxEntry::new(read_only.clone().into(), FileSystemAccessMode::Read),
    ]);
    // Compare the process's start time as well as its PID: a PID from the parent
    // namespace may be assigned to an unrelated process in an isolated child.
    let script = r#"
import os, sys
from pathlib import Path
pid, parent_stat, parent_fd, expected = sys.argv[1:]
def identity(stat):
    return stat.rsplit(')', 1)[1].split()[19]
try:
    visible = identity(Path(f'/proc/{pid}/stat').read_text()) == identity(parent_stat)
except FileNotFoundError:
    visible = False
assert visible == (expected == 'host'), (visible, expected)
if visible:
    os.kill(int(pid), 0)
    denied = Path.cwd() / 'denied'
    for path in [Path(f'/proc/{pid}/root') / str(denied).lstrip('/'),
                 Path(f'/proc/{pid}/fd/{parent_fd}')]:
        try:
            path.read_text()
        except PermissionError:
            pass
        else:
            raise AssertionError(f'host process filesystem accessible: {path}')
for path, mode in [('denied', 'r'), ('read-only', 'w')]:
    try:
        open(path, mode).close()
    except PermissionError:
        pass
    except OSError as error:
        assert error.errno == 30, error
    else:
        raise AssertionError(f'filesystem restriction lost: {path}')
Path('writable').write_text('allowed')
print(expected)
"#;
    for network in [
        NetworkSandboxPolicy::Restricted,
        NetworkSandboxPolicy::Enabled,
    ] {
        for (namespace, expected) in [
            (PidNamespace::Isolated, "isolated"),
            (PidNamespace::Host, "host"),
        ] {
            let profile = PermissionProfile::from_runtime_permissions(&file_system, network)
                .with_pid_namespace(namespace);
            let output = spawn_command_under_sandbox(
                vec![
                    python.to_string_lossy().into_owned(),
                    "-c".into(),
                    script.into(),
                    pid.to_string(),
                    parent_stat.clone(),
                    parent_file.as_raw_fd().to_string(),
                    expected.into(),
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
                "{namespace:?} / {network:?}: {output:?}"
            );
            assert_eq!(String::from_utf8(output.stdout)?, format!("{expected}\n"));
        }
    }
    assert_eq!(
        (
            std::fs::read_to_string(denied)?,
            std::fs::read_to_string(read_only)?,
            std::fs::read_to_string(workspace.join("writable"))?
        ),
        (
            "denied fixture".to_string(),
            "read-only fixture".to_string(),
            "allowed".to_string()
        ),
    );
    Ok(())
}
