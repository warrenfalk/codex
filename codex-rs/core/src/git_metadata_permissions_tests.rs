use super::*;
use codex_exec_server::LOCAL_FS;
use codex_protocol::permissions::NetworkSandboxPolicy;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn workspace_profile() -> PermissionProfile {
    PermissionProfile::workspace_write_with(
        &[],
        NetworkSandboxPolicy::Restricted,
        /*exclude_tmpdir_env_var*/ true,
        /*exclude_slash_tmp*/ true,
    )
}

fn absolute(path: &Path) -> AbsolutePathBuf {
    AbsolutePathBuf::from_absolute_path(path.canonicalize().expect("canonicalize path"))
        .expect("canonical path should be absolute")
}

fn run_git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed with {status}");
}

fn init_repo() -> TempDir {
    let temp = tempfile::tempdir().expect("create temp directory");
    run_git(temp.path(), &["init", "--quiet"]);
    run_git(temp.path(), &["config", "user.name", "Codex Test"]);
    run_git(temp.path(), &["config", "user.email", "codex@example.com"]);
    fs::write(temp.path().join("tracked.txt"), "initial\n").expect("write fixture");
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "--quiet", "-m", "initial"]);
    temp
}

#[tokio::test]
async fn linked_worktree_reopens_only_its_private_gitdir() {
    let main_repo = init_repo();
    let worktree_path = main_repo.path().join("linked");
    run_git(
        main_repo.path(),
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked-test",
            worktree_path.to_str().expect("UTF-8 worktree path"),
        ],
    );
    let cwd = absolute(&worktree_path);
    let profile =
        with_git_metadata_write_access(LOCAL_FS.as_ref(), &cwd, workspace_profile()).await;
    let policy = profile.file_system_sandbox_policy();
    let common_git_dir = absolute(&main_repo.path().join(".git"));
    let pointer = fs::read_to_string(worktree_path.join(".git")).expect("read .git pointer");
    let private_path = pointer
        .trim()
        .strip_prefix("gitdir:")
        .expect("gitdir pointer")
        .trim();
    let private_git_dir = absolute(Path::new(private_path));

    assert!(!policy.can_write_path_with_cwd(cwd.join(".git").as_path(), cwd.as_path()));
    assert!(policy.can_write_path_with_cwd(
        common_git_dir.join("refs/heads/linked-test").as_path(),
        cwd.as_path(),
    ));
    assert!(
        policy.can_write_path_with_cwd(private_git_dir.join("index").as_path(), cwd.as_path(),)
    );
    assert!(
        policy.can_write_path_with_cwd(private_git_dir.join("logs/HEAD").as_path(), cwd.as_path(),)
    );
    for relative_path in ["commondir", "gitdir", "config.worktree"] {
        assert!(
            !policy.can_write_path_with_cwd(
                private_git_dir.join(relative_path).as_path(),
                cwd.as_path(),
            )
        );
    }
    assert!(
        !policy.can_write_path_with_cwd(common_git_dir.join("config").as_path(), cwd.as_path(),)
    );
}

#[tokio::test]
async fn read_only_project_does_not_gain_git_writes() {
    let repo = init_repo();
    let cwd = absolute(repo.path());
    let original = PermissionProfile::read_only();
    let profile = with_git_metadata_write_access(LOCAL_FS.as_ref(), &cwd, original.clone()).await;

    assert_eq!(profile, original);
}

#[tokio::test]
async fn empty_git_directory_does_not_gain_git_writes() {
    let temp = tempfile::tempdir().expect("create temp directory");
    fs::create_dir(temp.path().join(".git")).expect("create empty .git directory");
    let cwd = absolute(temp.path());
    let original = workspace_profile();
    let profile = with_git_metadata_write_access(LOCAL_FS.as_ref(), &cwd, original.clone()).await;

    assert_eq!(profile, original);
}

#[tokio::test]
async fn forged_worktree_pointer_does_not_gain_git_writes() {
    let temp = tempfile::tempdir().expect("create temp directory");
    let repo = temp.path().join("repo");
    let target = temp.path().join("target/.git/worktrees/forged");
    fs::create_dir_all(&repo).expect("create repo");
    fs::create_dir_all(&target).expect("create forged private gitdir");
    fs::write(repo.join(".git"), format!("gitdir: {}\n", target.display()))
        .expect("write forged pointer");
    fs::write(target.join("commondir"), "../..\n").expect("write commondir");
    fs::write(target.join("gitdir"), "/not/the/repo/.git\n").expect("write reverse pointer");

    let cwd = absolute(&repo);
    let original = workspace_profile();
    let profile = with_git_metadata_write_access(LOCAL_FS.as_ref(), &cwd, original.clone()).await;

    assert_eq!(profile, original);
}

#[tokio::test]
async fn overlay_is_recomputed_for_each_repo_cwd() {
    let first_repo = init_repo();
    let second_repo = init_repo();
    let first_cwd = absolute(first_repo.path());
    let second_cwd = absolute(second_repo.path());

    let first_profile =
        with_git_metadata_write_access(LOCAL_FS.as_ref(), &first_cwd, workspace_profile()).await;
    let second_profile =
        with_git_metadata_write_access(LOCAL_FS.as_ref(), &second_cwd, workspace_profile()).await;

    assert!(
        first_profile
            .file_system_sandbox_policy()
            .can_write_path_with_cwd(first_cwd.join(".git/index").as_path(), first_cwd.as_path(),)
    );
    assert!(
        !first_profile
            .file_system_sandbox_policy()
            .can_write_path_with_cwd(second_cwd.join(".git/index").as_path(), first_cwd.as_path(),)
    );
    assert!(
        second_profile
            .file_system_sandbox_policy()
            .can_write_path_with_cwd(
                second_cwd.join(".git/index").as_path(),
                second_cwd.as_path(),
            )
    );
    assert!(
        !second_profile
            .file_system_sandbox_policy()
            .can_write_path_with_cwd(first_cwd.join(".git/index").as_path(), second_cwd.as_path(),)
    );
}
