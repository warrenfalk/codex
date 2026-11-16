//! Keep cleanup records writable to the launcher and hidden from its child.
//! A nested launcher can use its enclosing sandbox's private runtime directory.

use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

pub(crate) fn synthetic_mount_registry_root() -> PathBuf {
    static REGISTRY_ROOT: OnceLock<PathBuf> = OnceLock::new();

    REGISTRY_ROOT
        .get_or_init(|| {
            let effective_uid = unsafe { libc::geteuid() };
            let temp_dir = std::env::temp_dir();
            let temp_dir = temp_dir.canonicalize().unwrap_or_else(|err| {
                panic!(
                    "failed to resolve synthetic mount registry temp directory {}: {err}",
                    temp_dir.display()
                )
            });
            let registry_root = temp_dir.join(format!(
                "codex-bwrap-synthetic-mount-targets-{effective_uid}"
            ));
            // Reject redirection before probing the usual registry's lock.
            assert!(
                !registry_root.is_symlink(),
                "synthetic mount registry must not be a symlink: {}",
                registry_root.display()
            );
            let lock = fs::create_dir_all(&registry_root).and_then(|()| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(registry_root.join("lock"))
            });
            match lock {
                Ok(_) => registry_root,
                Err(err) if err.kind() == io::ErrorKind::ReadOnlyFilesystem => {
                    // The parent keeps its registry read-only. Its private
                    // runtime is writable here and hidden again from our child.
                    let private_root = codex_uds::prepare_shared_daemon_socket_directory()
                        .unwrap_or_else(|err| {
                            panic!("failed to prepare private sandbox runtime: {err}")
                        })
                        .join("bwrap-synthetic-mount-targets");
                    assert!(
                        !private_root.is_symlink(),
                        "synthetic mount registry must not be a symlink: {}",
                        private_root.display()
                    );
                    private_root
                }
                Err(err) => panic!(
                    "failed to prepare synthetic bubblewrap mount registry {}: {err}",
                    registry_root.display()
                ),
            }
        })
        .clone()
}

#[cfg(test)]
#[path = "synthetic_mount_registry_tests.rs"]
mod tests;
