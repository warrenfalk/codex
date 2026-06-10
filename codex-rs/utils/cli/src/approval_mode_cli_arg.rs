//! Standard type to use with the `--approval-mode` CLI option.

use clap::ValueEnum;

use codex_protocol::protocol::AskForApproval;

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ApprovalModeCliArg {
    /// Only run "trusted" commands (e.g. ls, cat, sed) without asking for user
    /// approval. Will escalate to the user if the model proposes a command that
    /// is not in the "trusted" set.
    Untrusted,

    /// DEPRECATED: Run all commands without asking for user approval.
    /// Only asks for approval if a command fails to execute, in which case it
    /// will escalate to the user to ask for un-sandboxed execution.
    /// Prefer `on-request` for interactive runs or `never` for non-interactive runs.
    OnFailure,

    /// The model decides when to ask the user for approval.
    OnRequest,

    /// Like `on-request`, but dangerous commands run inside a managed
    /// restricted sandbox without prompting.
    TrustSandbox,

    /// Like `trust-sandbox`, but sandbox-override prompts can auto-approve
    /// after the configured timeout.
    TrustSandboxTimeout,

    /// Never ask for user approval
    /// Execution failures are immediately returned to the model.
    Never,
}

impl From<ApprovalModeCliArg> for AskForApproval {
    fn from(value: ApprovalModeCliArg) -> Self {
        match value {
            ApprovalModeCliArg::Untrusted => AskForApproval::UnlessTrusted,
            ApprovalModeCliArg::OnFailure => AskForApproval::OnFailure,
            ApprovalModeCliArg::OnRequest => AskForApproval::OnRequest,
            ApprovalModeCliArg::TrustSandbox => AskForApproval::TrustSandbox,
            ApprovalModeCliArg::TrustSandboxTimeout => AskForApproval::TrustSandboxTimeout,
            ApprovalModeCliArg::Never => AskForApproval::Never,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;

    #[test]
    fn trust_sandbox_cli_values_map_to_core_policy() {
        assert_eq!(
            AskForApproval::from(
                ApprovalModeCliArg::from_str("trust-sandbox", /*ignore_case*/ false)
                    .expect("trust-sandbox value")
            ),
            AskForApproval::TrustSandbox,
        );
        assert_eq!(
            AskForApproval::from(
                ApprovalModeCliArg::from_str("trust-sandbox-timeout", /*ignore_case*/ false)
                    .expect("trust-sandbox-timeout value")
            ),
            AskForApproval::TrustSandboxTimeout,
        );
    }
}
