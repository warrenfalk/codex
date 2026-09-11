use crate::ApprovalPromptContext;
use crate::PermissionsInstructions;
use codex_execpolicy::Decision;
use codex_execpolicy::Policy;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ApprovalMessages;
use codex_protocol::openai_models::PermissionMessages;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::FileSystemSpecialPath;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::GranularApprovalConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

#[test]
fn profile_instructions_replace_catalog_and_builtin_guidance_including_empty_text() {
    let approvals = ApprovalMessages {
        on_request: Some("Catalog approvals".to_string()),
        on_request_auto_review: Some("Catalog auto-review".to_string()),
        never: Some("Catalog never".to_string()),
        unless_trusted: Some("Catalog untrusted".to_string()),
    };
    let permissions = PermissionMessages {
        danger_full_access: None,
        workspace_write: Some("Catalog filesystem".to_string()),
        read_only: None,
    };
    let cwd = AbsolutePathBuf::from_absolute_path(std::env::temp_dir()).unwrap();
    let denied = AbsolutePathBuf::from_absolute_path(cwd.join("blocked")).unwrap();
    let glob = denied.join("**").to_string_lossy().into_owned();
    let profile = PermissionProfile::from_runtime_permissions(
        &FileSystemSandboxPolicy::restricted(vec![
            FileSystemSandboxEntry {
                path: FileSystemPath::Special {
                    value: FileSystemSpecialPath::Root,
                },
                access: FileSystemAccessMode::Read,
                missing_path_behavior: None,
            },
            FileSystemSandboxEntry {
                path: FileSystemPath::Path {
                    path: cwd.clone().into(),
                },
                access: FileSystemAccessMode::Write,
                missing_path_behavior: None,
            },
            FileSystemSandboxEntry {
                path: FileSystemPath::Path {
                    path: denied.clone().into(),
                },
                access: FileSystemAccessMode::Deny,
                missing_path_behavior: None,
            },
            FileSystemSandboxEntry {
                path: FileSystemPath::GlobPattern {
                    pattern: glob.clone(),
                },
                access: FileSystemAccessMode::Deny,
                missing_path_behavior: None,
            },
        ]),
        NetworkSandboxPolicy::Enabled,
    );
    let cwd_display = cwd.display();
    let denied_display = denied.display();
    let mut policy = Policy::empty();
    policy
        .add_prefix_rule(&["git".to_string(), "status".to_string()], Decision::Allow)
        .unwrap();
    for approval_policy in [
        AskForApproval::OnRequest,
        AskForApproval::TrustSandbox,
        AskForApproval::TrustSandboxTimeout,
        AskForApproval::Never,
        AskForApproval::UnlessTrusted,
        AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: false,
            rules: true,
            skill_approval: false,
            request_permissions: true,
            mcp_elicitations: false,
        }),
    ] {
        let approval_details = if matches!(approval_policy, AskForApproval::Granular(_)) {
            "\n\nApproval categories enabled: sandbox_approval=false, rules=true, skill_approval=false, request_permissions=true, mcp_elicitations=false."
        } else {
            ""
        };
        for instructions in ["Try inside the sandbox first.", ""] {
            let actual = PermissionsInstructions::from_permission_profile(
                &profile,
                approval_policy,
                ApprovalPromptContext::new(
                    ApprovalsReviewer::AutoReview,
                    Some(&approvals),
                    Some(&permissions),
                    Some(instructions),
                ),
                &policy,
                &cwd,
                /*exec_permission_approvals_enabled*/ true,
                /*request_permissions_tool_enabled*/ true,
            );
            let suffix = if instructions.is_empty() {
                String::new()
            } else {
                format!("\n\n{instructions}")
            };
            assert_eq!(
                actual.body(),
                format!(
                    "This permissions block replaces any earlier permissions block and remains active until a later permissions block replaces it.\n\n## Active permissions\n`sandbox_mode`: `workspace-write`\n`approval_policy`: `{approval_policy}`\n`approvals_reviewer`: `auto_review`\nNetwork access: enabled.{approval_details}\n\nThe writable root is `{cwd_display}`.\n\n## Denied filesystem reads\n- path `{denied_display}`\n- glob `{glob}`\n\n## Approved command prefixes\n- [\"git\", \"status\"]{suffix}\n"
                ),
            );
        }
    }
}
