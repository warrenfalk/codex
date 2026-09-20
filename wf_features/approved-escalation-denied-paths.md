# Approved escalation past denied filesystem paths

## Intent

Keep selected files inaccessible during ordinary agent work while letting the user authorize
commands that need broader access. An approved request to run outside the sandbox must actually
run outside it, including when the selected permission profile contains denied filesystem paths.

## Required behavior

- Both `deny` and its existing `none` alias block reads and writes during sandboxed execution.
- An approved full sandbox escalation bypasses those entries for that command and its process
  descendants. This also permits access to host processes that the sandbox normally hides.
- Intercepted child commands reuse an approved parent shell's full sandbox escalation. Explicit
  command rules can still require approval or forbid a child command.
- Approval does not remove or rewrite the selected profile's entries. Subsequent sandboxed
  commands remain restricted, including commands later in the same turn.
- Existing command approvals and saved allow rules retain their normal scope and lifetime. When
  they authorize execution outside the sandbox, denied filesystem paths do not prevent it.
- Input to an escalated terminal follows the existing terminal-input approval rules. Denied paths
  must not cause an unconditional rejection before that review can take place.
- Rejected requests and approval policies that disallow escalation do not grant access.
- Network-only approvals and additional permissions do not imply full sandbox escalation.
- Model instructions describe denied filesystem paths as sandbox restrictions that can be bypassed
  by approved escalation. They must not describe `deny` or `none` as permanently non-escalatable.
- The behavior applies to ordinary shell execution, unified execution, intercepted shell commands,
  and local or remote executors. Existing independent executor and network restrictions still apply.
- No new access mode or configuration flag is required. A non-overridable denial mode is outside
  this feature's scope.

## Validation expectations

- Exercise both `deny` and `none` using temporary fixture files: normal access fails, approved
  escalation succeeds, and a later sandboxed read fails again.
- Verify that declining escalation prevents the command from reading the fixture.
- Cover direct execution and intercepted shell execution, including saved command approvals.
- Exercise actual Linux sandbox enforcement and ensure generated model instructions agree with
  runtime behavior.
