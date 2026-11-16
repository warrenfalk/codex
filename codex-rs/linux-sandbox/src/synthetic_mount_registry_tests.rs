use super::synthetic_mount_registry_root;
use pretty_assertions::assert_eq;
use std::io::Write;

#[test]
fn registry_remains_writable_in_the_launcher_namespace() -> std::io::Result<()> {
    let root = synthetic_mount_registry_root();
    std::fs::create_dir_all(&root)?;
    let mut record = tempfile::NamedTempFile::new_in(root)?;
    record.write_all(b"cleanup state")?;
    assert_eq!(std::fs::read(record.path())?, b"cleanup state");
    Ok(())
}
