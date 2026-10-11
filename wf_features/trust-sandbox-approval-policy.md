# Trust Sandbox Approval Policy

## Intent

`trust-sandbox` and `trust-sandbox-timeout` reduce approval prompts for commands whose risk is already bounded by the active sandbox. They are intended for fork-local workflows where the user wants Codex to keep momentum on sandboxed command execution without turning off sandbox enforcement.

## Behavior

Both policies are selectable through config and the `--ask-for-approval` CLI option:

- `trust-sandbox`
- `trust-sandbox-timeout`

They behave like `on-request` for ordinary command execution, except fallback dangerous-command heuristics do not prompt when the command will run inside a managed restricted filesystem sandbox. For example, command shapes such as `rm -f` or `rm -rf` may run without a prompt when the effective filesystem sandbox is restricted and managed by Codex.

The sandbox remains the safety boundary. These policies must not disable sandboxing or run a command outside the sandbox merely because the command shape was trusted.

## Approval Boundaries

Sandbox overrides still require command approval. This includes requests to run without sandbox restrictions and requests that need additional sandbox permissions.

Explicit exec-policy rules keep their normal priority:

- A matching `prompt` rule still prompts.
- A matching `forbidden` rule is still rejected.
- A matching allow rule keeps its existing behavior.

The policies do not change file-change approvals, MCP approvals, standalone permission-request approvals, network-policy approvals, or any non-command approval flow.

## Timeout Behavior

`trust-sandbox-timeout` adds a timeout only for sandbox-override command approval prompts. If the user does not respond within 300 seconds, that single approval request is treated as approved.

Timeout approval is one-time for the pending request. It must not persist an approval for the session, write or accept an exec-policy amendment, or write or accept a network-policy amendment. If the user responds before the timeout, the user's decision wins. If the turn is interrupted or cancelled before the timeout, existing abort and denial behavior applies.

`trust-sandbox` has no auto-approval timeout.

## UI Expectations

The policies should render by their exact names anywhere the current approval policy is displayed. The TUI permissions picker does not need new presets for the initial version; config and CLI selection are sufficient.

## Validation

Validation should cover dangerous command shapes running sandboxed under a managed restricted sandbox, dangerous command shapes still prompting without such a sandbox, explicit sandbox overrides prompting under both policies, explicit exec-policy prompt rules taking priority over timeout behavior, and timeout approval applying only to sandbox-override command prompts after 300 seconds.

Protocol, config, CLI, generated schema, and status rendering validation should confirm both policy names round-trip and display correctly.
