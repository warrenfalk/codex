use super::PermissionsRenderContext;
use super::approved_command_prefixes_text;
use super::writable_roots_text;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::protocol::AskForApproval;

pub(super) fn render(
    context: PermissionsRenderContext<'_>,
    reviewer: ApprovalsReviewer,
    instructions: &str,
) -> String {
    let PermissionsRenderContext {
        sandbox_mode,
        network_access,
        approval_policy,
        approved_command_prefixes,
        writable_roots,
        denied_read_paths,
        denied_read_globs,
        ..
    } = context;
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
    let denied_reads = denied_read_paths
        .iter()
        .map(|path| format!("- path `{path}`"))
        .chain(
            denied_read_globs
                .iter()
                .map(|glob| format!("- glob `{glob}`")),
        )
        .collect::<Vec<_>>();
    if !denied_reads.is_empty() {
        sections.push(format!(
            "## Denied filesystem reads\n{}",
            denied_reads.join("\n")
        ));
    }
    if let Some(prefixes) = approved_command_prefixes_text(approved_command_prefixes) {
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
