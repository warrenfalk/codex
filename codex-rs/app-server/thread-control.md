# External thread control API

This fork lets an external MCP bridge read Codex threads and request permission to
send input or interrupt turns. The MCP server and its authentication live outside
this repository. Connect it to the **same app-server process** used by your GUI or
TUI. A separate app-server pointed at the same data directory cannot observe the
other process's active turns, attachments, or grants.

## Connection identity and trust

Authenticate Dot at your MCP boundary and map it to the stable ID `dot`. Use a
dedicated app-server connection for that identity. All Dot tasks and reconnects
share this identity; another agent such as Claude should have a different ID and
connection. Never accept an arbitrary controller ID from a model's tool arguments.

The app-server is a trusted local API, not a new authentication boundary.
`controller/register` voluntarily restricts a connection; it does not authenticate
it. The bridge must keep the underlying unrestricted app-server connection private,
register before exposing tools, and never forward arbitrary RPCs from agents.
Existing interactive clients retain their normal authority. An untrusted client
that can directly open an unrestricted app-server connection can bypass this policy.

Perform the normal `initialize` handshake and send `initialized`. Your first
request after initialization must be:

```json
{"id":2,"method":"controller/register","params":{"controllerId":"dot"}}
```

The result is `{}`. Registration is immutable for that connection. A connection
that has already made another request cannot register later. IDs are case-sensitive,
1–128 bytes, and cannot contain control characters.

After registration the allowed methods are:

- `event/firehose`
- `thread/list`, `thread/loaded/list`, `thread/read`
- `thread/turns/list`, `thread/items/list`, `thread/items/read`
- `thread/resume`, `thread/unsubscribe`
- `thread/control/read`, `thread/control/request`, `thread/control/release`
- `thread/control/input`, `thread/control/interrupt`

`thread/resume` accepts only `threadId`, `excludeTurns`, and `initialTurnsPage`;
configuration overrides and history/path replacement are forbidden. Resume a saved
thread before sending input. Controller subscriptions do not count as attached
interactive interfaces. They cannot receive answerable server requests, including
pending approvals replayed on resume, and cannot answer approvals.

## Observe and request control

Read access does not require a grant. Use existing history pagination to inspect
the task, and `event/firehose` to observe live activity. Firehose observation itself
does not load a thread or create an interactive attachment.

```json
{"id":3,"method":"event/firehose"}
{"id":4,"method":"thread/read","params":{"threadId":"THREAD_ID"}}
{"id":5,"method":"thread/control/request","params":{"threadId":"THREAD_ID","requestId":"UNIQUE_REQUEST_ID","reason":"Finish the build you asked me to check."}}
```

The request returns immediately with `{"state": ...}`. Use a fresh unique request
ID for each new attempt. Repeating the **latest** request ID with the same caller
and reason returns its current state without resetting its countdown or changing
its decision. Only the latest request is retained; do not replay older attempts
after a newer request. Reasons are at most 1,024 bytes, with no terminal control
characters except newlines.

Requesting control, locking, or granting in advance saves an otherwise empty
durable thread so it remains resumable after its last interactive client detaches.

Example pending state:

```json
{
  "state": {
    "threadId": "THREAD_ID",
    "locked": false,
    "grant": null,
    "request": {
      "id": "UNIQUE_REQUEST_ID",
      "controllerId": "dot",
      "reason": "Finish the build you asked me to check.",
      "status": "pending",
      "requestedAt": 1800000000,
      "autoGrantAt": 1800000060
    },
    "interactiveAttached": true,
    "activeTurnId": "TURN_ID",
    "turnRunning": true
  }
}
```

Rules:

- Idle, unattached, unlocked threads grant immediately.
- An interactive attachment **or** a running turn causes a full 60-second wait,
  unless the user explicitly approves or denies sooner.
- Locked threads require explicit approval and have `autoGrantAt: null`.
- Only one controller may hold a grant, and only one request may be pending.
  Another controller gets an error and must wait; requests are not queued.
- A request from the existing holder returns its current grant.
- A denied request remains denied. A new attempt requires a new request ID and
  gets a fresh countdown. Agents should respect denial and retry only when asked
  or when circumstances materially change.

Subscribe to `thread/control/changed`, whose params are `{"state": ...}`, or poll:

```json
{"id":6,"method":"thread/control/read","params":{"threadId":"THREAD_ID"}}
```

Notifications are broadcast, including for background threads. Read a fresh
snapshot after reconnecting. Timestamps are Unix seconds; the server's timeout is
measured by its monotonic clock. **The server owns the timeout**; never infer a
grant from your own clock. A granted request has `status: "granted"` and a non-null
`grant: {"id":"GRANT_ID","controllerId":"dot"}`. The grant itself is authoritative:
the latest request can remain marked granted after its grant has been revoked.

## Send input and interrupt

Every mutation requires the current grant ID and matching registered identity.
Grants cover multiple turns until revoked or released. Reconnects preserve grants
within the lifetime of the same app-server process.

To start a turn, load the thread if necessary and omit `expectedTurnId`:

```json
{"id":7,"method":"thread/resume","params":{"threadId":"THREAD_ID","excludeTurns":true}}
{"id":8,"method":"thread/control/input","params":{"threadId":"THREAD_ID","grantId":"GRANT_ID","input":[{"type":"text","text":"Continue the build and report its result.","text_elements":[]}]}}
```

Input returns `{"turnId":"NEW_TURN_ID"}` once accepted. It does not wait for turn
completion. Starting requires the thread to be idle; it cannot silently steer a
turn that started concurrently. To deliberately add input to a running turn:

```json
{"id":9,"method":"thread/control/input","params":{"threadId":"THREAD_ID","grantId":"GRANT_ID","expectedTurnId":"ACTIVE_TURN_ID","input":[{"type":"text","text":"Also include the failing test output.","text_elements":[]}]}}
```

Input uses the existing `UserInput` wire format, including its compatibility key
`text_elements`; the control API's own fields use camelCase.

This requires that exact turn still be active and steerable. Input never implicitly
interrupts. If the grant response says `turnRunning: true`, the agent should
normally wait and observe completion. It may explicitly interrupt when appropriate:

```json
{"id":10,"method":"thread/control/interrupt","params":{"threadId":"THREAD_ID","grantId":"GRANT_ID","turnId":"ACTIVE_TURN_ID"}}
```

An explicit non-empty turn ID is required, and the existing interrupt preconditions
apply. The result is `{}`. `turnRunning` can be true while `activeTurnId` is null
during startup; wait for a usable turn ID instead of guessing.

Controller input accepts no model, sandbox, reviewer, approval-policy, or other
configuration overrides. Unknown input fields are rejected. Normal `turn/start`,
`turn/steer`, `turn/interrupt`, configuration changes, command execution, and
human control actions are unavailable to controller connections.

Existing input restrictions still apply: a thread whose `canAcceptDirectInput`
is false, such as a parent-owned subagent, still rejects controller input.

Never automatically retry an input or interrupt after an ambiguous transport
failure: it may already have been accepted. Reconcile history and turn status.

## Human control and interface integration

Interactive clients use `thread/control/update`:

```json
{"id":20,"method":"thread/control/update","params":{"threadId":"THREAD_ID","action":{"type":"approve","requestId":"UNIQUE_REQUEST_ID"}}}
```

Actions:

| Action | Fields | Effect |
| --- | --- | --- |
| `approve` | `requestId` | Grant the exact pending request; keep any lock |
| `deny` | `requestId` | Deny that pending request; do not lock |
| `revoke` | none | Clear the current grant and deny any pending request |
| `lock` | none | Persist a lock, revoke the grant, and deny any pending request |
| `unlock` | none | Remove the lock; an already-pending explicit-only request stays explicit-only |
| `grant` | `controllerId` | Grant access in advance, without removing the lock |

Granting another identity requires revoking the current holder first. Approving
or denying a stale request ID fails instead of acting on a newer request.

The TUI exposes these through `/control`, `/control approve`, `/control deny`,
`/control revoke`, `/control lock`, `/control unlock`, and `/control grant dot`.
Its control notice stays above the composer during turns and dialogs. Permission
notifications use the existing `approval-requested` notification category and
honor notification settings.

A separate GUI must implement this contract:

1. Read control state when attaching/reconnecting, and consume change notifications.
2. Immediately show pending requests at the bottom, including controller, reason,
   deadline or explicit-only status, and approve/deny controls. Generate a notification.
3. Keep the holder and lock state visible. Preserve drafts and running turns.
4. Use the exact displayed request ID for approval/denial.
5. Human `turn/start`, `turn/steer`, and server queue submissions automatically
   revoke control and deny pending requests. If input is queued **locally**, send
   `revoke` at user submission time, before retaining the local queue item. Typing
   alone must not revoke access.

The running turn continues after revocation. Your interrupt action remains
available. Interrupting alone does not revoke a grant; use `revoke` or `lock` to
prevent further controller commands as well. A controller's old grant ID stops
working even if it later obtains a new grant. Approval prompts remain human-owned;
control does not delegate approval authority. Deploy GUI support before relying on
the one-minute notice in that GUI.

## Release, restart, and failures

When finished:

```json
{"id":30,"method":"thread/control/release","params":{"threadId":"THREAD_ID","grantId":"GRANT_ID"}}
```

The response contains the resulting state. Release does not interrupt ongoing work.
Disconnection alone does not release a grant. Restarting app-server clears all
grants and requests; locks persist under `$CODEX_HOME/thread-control-locks/`.

Missing/stale grants, wrong identities, competing controllers, stale pending
request IDs, and forbidden controller methods produce JSON-RPC errors. Re-read
state to recover. A timeout or a lost connection never implies permission.

The bridge should expose short MCP operations for requesting control, reading
status, submitting input, interrupting, and releasing. Waiting for human approval
or turn completion should use subsequent calls rather than holding one tunnel
request open.
