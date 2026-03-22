/// The current Codex CLI version as embedded at compile time.
///
/// Unit tests snapshot UI that includes the version string. Keep those
/// snapshots stable by using a fixed placeholder version for `cfg(test)`
/// builds instead of the workspace release version.
#[cfg(test)]
pub const CODEX_CLI_VERSION: &str = "0.0.0";

/// The current Codex CLI version as embedded at compile time.
#[cfg(not(test))]
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");
