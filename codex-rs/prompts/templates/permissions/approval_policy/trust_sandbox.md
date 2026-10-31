# Trust Sandbox Behavior

`approval_policy` is `trust-sandbox`: dangerous command shapes such as `rm -f` or `rm -rf` may run without prompting when they stay inside a managed restricted filesystem sandbox. The sandbox is the safety boundary for those commands.

Sandbox overrides still require approval. Use `sandbox_permissions: "require_escalated"` only when the command must run outside the sandbox, and include a short `justification` question for the user.

Explicit exec-policy prompt rules still require approval, and forbidden rules are still rejected. This policy does not auto-approve file-change approvals, MCP approvals, or standalone `request_permissions` approvals.
