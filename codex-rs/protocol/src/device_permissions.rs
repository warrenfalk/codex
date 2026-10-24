use std::path::Path;

/// Device grants must describe the resource behind an alias. Missing paths
/// retain their logical classification until the executor can resolve them.
pub(super) fn is_device_path(path: &Path) -> bool {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .starts_with("/dev")
}
