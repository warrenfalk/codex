use super::*;
use pretty_assertions::assert_eq;

#[test]
fn device_grants_preserve_explicit_overrides_without_implicit_repository_masks() {
    let devices = AbsolutePathBuf::try_from("/dev/codex-test-devices").unwrap();
    let workspace = AbsolutePathBuf::try_from("/codex-test-workspace").unwrap();
    let policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(devices.clone().into(), FileSystemAccessMode::Write),
        FileSystemSandboxEntry::new(workspace.clone().into(), FileSystemAccessMode::Write),
        FileSystemSandboxEntry::new(devices.join(".git").into(), FileSystemAccessMode::Read),
        FileSystemSandboxEntry::new(devices.join(".agents").into(), FileSystemAccessMode::Deny),
    ]);

    assert_eq!(
        [
            policy.can_write_local_path_with_cwd(
                devices.join("render-node").as_path(),
                workspace.as_path()
            ),
            policy.can_write_local_path_with_cwd(
                devices.join(".codex").as_path(),
                workspace.as_path()
            ),
            policy
                .can_write_local_path_with_cwd(devices.join(".git").as_path(), workspace.as_path()),
            policy.can_read_local_path_with_cwd(
                devices.join(".agents").as_path(),
                workspace.as_path()
            ),
            policy.can_write_local_path_with_cwd(
                workspace.join(".git").as_path(),
                workspace.as_path()
            ),
        ],
        [true, true, false, false, false],
    );
    let device_root = policy
        .get_writable_roots_with_cwd(workspace.as_path())
        .into_iter()
        .find(|root| root.root == devices)
        .unwrap();
    assert_eq!(
        device_root,
        WritableRoot {
            root: devices.clone(),
            read_only_subpaths: vec![devices.join(".git"), devices.join(".agents")],
            protected_metadata_names: Vec::new(),
        },
    );
}

#[test]
fn device_symlink_grants_do_not_infer_repository_masks() {
    let temp = tempfile::tempdir().unwrap();
    let device_link = AbsolutePathBuf::from_absolute_path(temp.path().join("device")).unwrap();
    std::os::unix::fs::symlink("/dev/null", &device_link).unwrap();
    let policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry::new(
        device_link.clone().into(),
        FileSystemAccessMode::Write,
    )]);

    assert!(policy.can_write_local_path_with_cwd(device_link.join(".git").as_path(), temp.path()));
    assert_eq!(
        policy.get_writable_roots_with_cwd(temp.path()),
        vec![WritableRoot {
            root: device_link,
            read_only_subpaths: Vec::new(),
            protected_metadata_names: Vec::new(),
        }],
    );
}
