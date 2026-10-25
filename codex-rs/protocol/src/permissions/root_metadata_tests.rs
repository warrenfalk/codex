//! Filesystem roots do not acquire automatic project metadata protections.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn root_grants_preserve_explicit_restrictions_without_protecting_metadata() {
    for location in [
        "file:///work/repo",
        "file:///C:/work/repo",
        "file://server/share/repo",
    ] {
        let cwd = PathUri::parse(location).unwrap();
        let context = FileSystemSandboxPolicyContext {
            cwd: &cwd,
            workspace_roots: std::slice::from_ref(&cwd),
            user_home_dir: None,
            temporary_directories: Some(&[]),
        };
        let root = file_system_root(&context).unwrap();
        let read_only = root.join(".git/config").unwrap();
        let denied = root.join(".codex/private").unwrap();
        let policy = FileSystemSandboxPolicy::restricted(vec![
            FileSystemSandboxEntry::new(root.clone().into(), FileSystemAccessMode::Write),
            FileSystemSandboxEntry::new(read_only.clone().into(), FileSystemAccessMode::Read),
            FileSystemSandboxEntry::new(denied.clone().into(), FileSystemAccessMode::Deny),
            FileSystemSandboxEntry::new(cwd.clone().into(), FileSystemAccessMode::Write),
        ]);

        assert_eq!(
            PROTECTED_METADATA_PATH_NAMES
                .iter()
                .map(|name| policy.can_write_path(&root.join_descendant(name).unwrap(), &context))
                .collect::<Vec<_>>(),
            vec![true; 4],
            "{location}",
        );
        assert_eq!(
            [read_only, denied].map(|path| (
                policy.can_read_path(&path, &context),
                policy.can_write_path(&path, &context),
            )),
            [(true, false), (false, false)],
            "{location}",
        );
        assert!(!policy.can_write_path(&cwd.join(".git/config").unwrap(), &context));
    }
}

#[test]
fn native_root_mounts_only_retain_explicit_read_only_subpaths() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().canonicalize().unwrap();
    let root = AbsolutePathBuf::from_absolute_path(cwd.ancestors().last().unwrap()).unwrap();
    let read_only = AbsolutePathBuf::from_absolute_path(cwd.join("read-only")).unwrap();
    let policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(root.clone().into(), FileSystemAccessMode::Write),
        FileSystemSandboxEntry::new(read_only.clone().into(), FileSystemAccessMode::Read),
    ]);
    for policy_cwd in [cwd.as_path(), root.as_path()] {
        assert_eq!(
            policy.get_writable_roots_with_cwd_inheriting_root_metadata(policy_cwd),
            vec![WritableRoot {
                root: root.clone(),
                read_only_subpaths: vec![read_only.clone()],
                protected_metadata_names: Vec::new(),
            }],
        );
    }
}
