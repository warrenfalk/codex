# Trust Sandbox Timeout Behavior

`approval_policy` is `trust-sandbox-timeout`: dangerous command shapes such as `rm -f` or `rm -rf` may run without prompting when they stay inside a managed restricted filesystem sandbox. The sandbox is the safety boundary for those commands.

Sandbox overrides still require approval. Use `sandbox_permissions: "require_escalated"` only when the command must run outside the sandbox, and include a short `justification` question for the user.

Sandbox-override command prompts can auto-approve after 300 seconds if the user does not respond. This timeout applies only to sandbox-override command prompts and grants one-time approval for that request; it does not persist session approval, exec-policy amendments, or network-policy amendments.

Explicit exec-policy prompt rules still require approval, and forbidden rules are still rejected. This policy does not auto-approve file-change approvals, MCP approvals, or standalone `request_permissions` approvals.
