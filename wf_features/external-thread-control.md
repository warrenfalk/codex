# External thread control

## Intent

Let an external agent, initially Dot through a separate MCP server, take control
of an existing Codex thread without surprising an attached user. The bridge can
observe threads freely, but mutations require revocable, thread-specific grants.

## Behavior to preserve

- Treat each authenticated bridge identity as one controller across tasks and
  reconnects. The bridge supplies a trusted stable identity, not the model.
- Only one external controller may hold control of a thread at a time. Competing
  requests fail rather than silently replacing the holder.
- Idle threads without an interactive attachment grant immediately when unlocked.
  An attached interface or a running turn requires a conspicuous permission request
  and notification. Silence grants access after a full minute.
- Locked threads require explicit approval. Locking also revokes current control
  and denies pending requests. Explicit approval or an advance grant does not
  remove the lock.
- Submission of human input immediately revokes control and denies pending
  requests, including input queued locally for later delivery. Typing does not.
  Running work continues; revocation never implicitly interrupts a turn.
- After revocation or denial the agent may make a fresh request, with a fresh
  minute unless locked. Agents should respect explicit denial.
- Grants cover multiple turns. Their responses report running-turn state and its
  ID when available. The controller should normally wait for active work, but may
  explicitly steer or interrupt an identified active turn when appropriate.
- Starting a controller turn must require idle state rather than accidentally
  steering a concurrently started turn. Old grant IDs cannot use later grants.
- Control does not delegate approval authority or permission/configuration changes.
  Controller attachments cannot receive or answer approvals, including replayed ones.
- Read access is independent of control. Observation must not count as human
  attachment or cause approval requests to become answerable by the observer.
- Disconnects do not revoke grants. Server restart clears grants and requests;
  locks survive restart. Reconnected interfaces recover current control state.
- Requesting control, locking, or granting access in advance keeps an empty durable
  thread resumable when its last interface detaches.
- The TUI keeps requests, active control, and locks visible at the bottom during
  streaming and dialogs. It offers `/control` commands without destroying drafts.
  Separate GUIs must implement the same API interaction and presentation contract.
- The external MCP service, its authentication, and its tunnel stay outside Codex.
  All clients coordinate through the same authoritative app-server process.

## Validation expectations

Exercise the public JSON-RPC API with independent interactive and controller
connections: idle immediate grants, attached/busy delayed grants, explicit decisions,
locks, advance grants, competing identities, reconnects, human mid-turn revocation,
pending-request denial by human input, stale grants, and persisted locks across
restart. Verify exact timeout timing and stale timer cancellation with a virtual
clock. Verify controller methods cannot bypass grants or change execution policies,
and approval delivery/replay cannot confer approval authority. Snapshot pending,
locked, and granted UI states and verify visibility during work with drafts intact.
