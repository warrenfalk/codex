#![cfg(target_os = "linux")]

use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::FileSystemSpecialPath;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn missing_read_only_git_lock_is_removed_after_supervisor_is_killed() {
    if !command_is_available("git") {
        eprintln!("skipping bwrap test: git is unavailable");
        return;
    }

    let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
    let repo = temp_dir.path().join("repo");
    std::fs::create_dir(&repo).expect("repository directory should be created");
    let git_init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init should run");
    assert!(git_init.success(), "git init should succeed");

    if !bubblewrap_is_available(&repo) {
        eprintln!("skipping bwrap test: bwrap sandbox prerequisites are unavailable");
        return;
    }

    let config = repo.join(".git/config");
    let config_lock = repo.join(".git/config.lock");
    let inside_lock_state = repo.join("inside-lock-state");
    let git_config_status = repo.join("git-config-status");
    let ready = repo.join("sandbox-ready");
    let permission_profile =
        git_control_permission_profile(&repo, &repo.join(".git"), &config, &config_lock);
    let mut child = sandbox_command(&repo, &permission_profile)
        .args([
            "--",
            "bash",
            "-lc",
            r#"if test -f .git/config.lock; then
  printf visible > inside-lock-state
else
  printf missing > inside-lock-state
fi
git config codex.syntheticMountProbe true 2>git-config.err
printf '%s' "$?" > git-config-status
: > sandbox-ready
while :; do sleep 0.01; done
"#,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sandbox helper should start");

    let command_started = wait_for_path(&ready);
    let lock_existed_while_running = config_lock.exists();
    child.kill().expect("sandbox supervisor should be killed");
    let output = child
        .wait_with_output()
        .expect("sandbox helper should finish");
    let lock_was_removed = wait_for_path_to_disappear(&config_lock);

    assert!(command_started, "sandbox command did not report readiness");
    assert_eq!(
        std::fs::read_to_string(&inside_lock_state)
            .expect("in-sandbox lock state should be recorded"),
        "visible"
    );
    assert_ne!(
        std::fs::read_to_string(&git_config_status).expect("git config status should be recorded"),
        "0"
    );
    assert!(
        lock_existed_while_running,
        "synthetic .git/config.lock was not present while the sandbox was running"
    );
    assert!(!output.status.success());
    assert!(
        lock_was_removed,
        "synthetic .git/config.lock survived a hard-killed sandbox supervisor"
    );
}

fn git_control_permission_profile(
    repo: &Path,
    git_dir: &Path,
    config: &Path,
    config_lock: &Path,
) -> PermissionProfile {
    let file_system_policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry {
            path: FileSystemPath::Special {
                value: FileSystemSpecialPath::Root,
            },
            access: FileSystemAccessMode::Read,
            missing_path_behavior: None,
        },
        path_entry(repo, FileSystemAccessMode::Write),
        path_entry(git_dir, FileSystemAccessMode::Write),
        path_entry(config, FileSystemAccessMode::Read),
        path_entry(config_lock, FileSystemAccessMode::Read),
    ]);
    PermissionProfile::from_runtime_permissions(&file_system_policy, NetworkSandboxPolicy::Enabled)
}

fn path_entry(path: &Path, access: FileSystemAccessMode) -> FileSystemSandboxEntry {
    FileSystemSandboxEntry {
        path: FileSystemPath::Path {
            path: AbsolutePathBuf::try_from(path)
                .expect("sandbox path should be absolute")
                .into(),
        },
        access,
        missing_path_behavior: None,
    }
}

fn sandbox_command(cwd: &Path, permission_profile: &PermissionProfile) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-linux-sandbox"));
    command
        .arg("--sandbox-policy-cwd")
        .arg(cwd)
        .arg("--permission-profile")
        .arg(
            serde_json::to_string(permission_profile).expect("permission profile should serialize"),
        )
        .arg("--no-proc")
        .current_dir(cwd);
    command
}

fn bubblewrap_is_available(cwd: &Path) -> bool {
    let file_system_policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
        path: FileSystemPath::Special {
            value: FileSystemSpecialPath::Root,
        },
        access: FileSystemAccessMode::Read,
        missing_path_behavior: None,
    }]);
    let permission_profile = PermissionProfile::from_runtime_permissions(
        &file_system_policy,
        NetworkSandboxPolicy::Enabled,
    );
    let output = sandbox_command(cwd, &permission_profile)
        .args(["--", "true"])
        .output()
        .expect("sandbox availability probe should run");
    if output.status.success() {
        return true;
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("bubblewrap is unavailable")
        || stderr.contains("Operation not permitted")
        || stderr.contains("Permission denied")
    {
        return false;
    }
    panic!(
        "sandbox availability probe failed unexpectedly:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
}

fn command_is_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wait_for_path(path: &Path) -> bool {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn wait_for_path_to_disappear(path: &Path) -> bool {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    while Instant::now() < deadline {
        if !path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
