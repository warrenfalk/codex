use super::*;
use crate::shell::Shell;
use crate::shell::ShellType;
use core_test_support::PathExt;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use std::process::Command;

fn shell_with_snapshot(
    shell_type: ShellType,
    shell_path: &str,
    snapshot_path: AbsolutePathBuf,
) -> (Shell, AbsolutePathBuf) {
    (
        Shell {
            shell_type,
            shell_path: PathBuf::from(shell_path),
        },
        snapshot_path,
    )
}

fn test_bash_path() -> String {
    if let Some(path) = std::env::var_os("BASH") {
        let path = PathBuf::from(path);
        if path.is_absolute() && path.is_file() {
            return path.to_string_lossy().to_string();
        }
    }

    let Some(path) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("bash"))
            .find(|path| path.is_file())
    }) else {
        panic!("bash should be available on PATH");
    };
    path.to_string_lossy().to_string()
}

#[test]
fn user_shell_snapshot_preserves_package_path_prepend() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let snapshot_path = dir.path().join("snapshot.sh");
    std::fs::write(
        &snapshot_path,
        "# Snapshot file\nexport PATH='/snapshot/bin'\n",
    )
    .expect("write snapshot");
    let bash_path = test_bash_path();
    let (session_shell, shell_snapshot) =
        shell_with_snapshot(ShellType::Bash, &bash_path, snapshot_path.abs());
    let command = vec![
        bash_path,
        "-lc".to_string(),
        "printf '%s' \"$PATH\"".to_string(),
    ];
    let package_path_dir = dir.path().join("codex-path");
    let mut env = HashMap::from([("PATH".to_string(), "/worktree/bin".to_string())]);
    let rewritten = prepare_user_shell_exec_command_with_path_prepend(
        &command,
        &session_shell,
        Some(&shell_snapshot),
        &HashMap::new(),
        &mut env,
        |env, runtime_path_prepends| {
            runtime_path_prepends.prepend(env, package_path_dir.as_path());
        },
    );
    let output = Command::new(&rewritten[0])
        .args(&rewritten[1..])
        .env("PATH", env.get("PATH").expect("PATH should be set"))
        .output()
        .expect("run rewritten command");

    assert!(output.status.success(), "command failed: {output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}:/snapshot/bin", package_path_dir.display())
    );
}

#[test]
fn user_shell_snapshot_preserves_project_env_path() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let snapshot_path = dir.path().join("snapshot.sh");
    std::fs::write(
        &snapshot_path,
        "# Snapshot file\nexport PATH='/snapshot/bin'\n",
    )
    .expect("write snapshot");
    let bash_path = test_bash_path();
    let (session_shell, shell_snapshot) =
        shell_with_snapshot(ShellType::Bash, &bash_path, snapshot_path.abs());
    let command = vec![
        bash_path,
        "-lc".to_string(),
        "printf '%s' \"$PATH\"".to_string(),
    ];
    let project_env_path = "/direnv/bin:/usr/bin";
    let mut env = HashMap::from([("PATH".to_string(), project_env_path.to_string())]);
    let snapshot_env_overrides =
        HashMap::from([("PATH".to_string(), project_env_path.to_string())]);
    let rewritten = prepare_user_shell_exec_command_with_path_prepend(
        &command,
        &session_shell,
        Some(&shell_snapshot),
        &snapshot_env_overrides,
        &mut env,
        |_env, _runtime_path_prepends| {},
    );
    let output = Command::new(&rewritten[0])
        .args(&rewritten[1..])
        .env("PATH", env.get("PATH").expect("PATH should be set"))
        .output()
        .expect("run rewritten command");

    assert!(output.status.success(), "command failed: {output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), project_env_path);
}
