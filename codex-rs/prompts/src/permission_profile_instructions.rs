use super::approved_command_prefixes_text;
use super::denied_read_entries;
use super::network_access_from_policy;
use super::sandbox_prompt_from_policy;
use super::writable_roots_text;
use codex_execpolicy::Policy;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use std::path::Path;

pub(super) fn render(
    permission_profile: &PermissionProfile,
    approval_policy: AskForApproval,
    reviewer: ApprovalsReviewer,
    exec_policy: &Policy,
    cwd: &Path,
    instructions: &str,
) -> String {
    let file_system = permission_profile.file_system_sandbox_policy();
    let (sandbox_mode, writable_roots) = sandbox_prompt_from_policy(&file_system, cwd);
    let network_access = network_access_from_policy(permission_profile.network_sandbox_policy());
    let mut sections = vec![
        "This permissions block replaces any earlier permissions block and remains active until a later permissions block replaces it.".to_string(),
        format!(
            "## Active permissions\n`sandbox_mode`: `{sandbox_mode}`\n`approval_policy`: `{approval_policy}`\n`approvals_reviewer`: `{reviewer}`\nNetwork access: {network_access}."
        ),
    ];
    if let AskForApproval::Granular(config) = approval_policy {
        let codex_protocol::protocol::GranularApprovalConfig {
            sandbox_approval,
            rules,
            skill_approval,
            request_permissions,
            mcp_elicitations,
        } = config;
        sections.push(format!(
            "Approval categories enabled: sandbox_approval={sandbox_approval}, rules={rules}, skill_approval={skill_approval}, request_permissions={request_permissions}, mcp_elicitations={mcp_elicitations}."
        ));
    }
    if let Some(roots) = writable_roots_text(writable_roots) {
        sections.push(roots.trim_start().to_string());
    }
    let denied_reads = denied_read_entries(&file_system, cwd);
    if !denied_reads.is_empty() {
        sections.push(format!(
            "## Denied filesystem reads\n{}",
            denied_reads.join("\n")
        ));
    }
    if let Some(prefixes) = approved_command_prefixes_text(exec_policy) {
        sections.push(format!("## Approved command prefixes\n{prefixes}"));
    }
    if !instructions.is_empty() {
        sections.push(instructions.to_string());
    }
    format!("{}\n", sections.join("\n\n"))
}

#[cfg(test)]
#[path = "permission_profile_instructions_tests.rs"]
mod tests;
