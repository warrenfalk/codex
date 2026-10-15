# Persistent Thread Acknowledgement

Sessions retain an unread flag so stopped work remains visible until the user acknowledges it.
Unread records an outstanding acknowledgement; it can stay set while an agent is working.

## Behavior

- An accepted user prompt sets the thread unread, including input delivered to an active turn.
- `/unread` explicitly sets unread without sending a prompt or starting work.
- `/ack` acknowledges the thread without sending a prompt or starting work.
- Every acknowledgement is ignored while the thread has an active turn, including waits for
  permission or user input, or while a descendant is active. The server enforces this for every client. Ignored attempts are
  discarded; they are not retried or applied when work stops.
- Opening an existing session from the agent list or successfully focusing it through
  `codex agents --focus` makes one acknowledgement attempt. Listing, reading, and receiving
  updates never acknowledge a session.
- Returning terminal focus to a displayed session makes one acknowledgement attempt after
  one continuous second of focus. Losing focus, suspending, disconnecting, switching to another
  session, or showing the agent list before the delay expires prevents that attempt. An attempt ignored
  during active work is not retried when work stops or while focus remains unchanged.
- The displayed session shows `unread` in bright blue in the footer while its unread flag is
  set and neither it nor a descendant has active work. The indicator disappears on acknowledgement
  or when work starts. It remains available even with no configured status-line items and takes
  priority over those items in narrow terminals. Ephemeral side conversations never show it.
- Turn completion, failure, interruption, and unloading leave unread unchanged. Acknowledgement
  survives server restarts and history reconciliation. Existing sessions begin acknowledged.
- Acknowledging or marking unread does not send input to the agent or change session recency.
- Unread is per thread. A subagent's unread flag does not mark its parent unread.
- Ephemeral side conversations do not acquire persistent unread state.
- Unread persistence failures never turn already accepted input into a rejected prompt.
  Explicit acknowledgement commands report persistence errors instead of claiming success.

## Lists and integrations

- The agent list prioritizes needs-input sessions, stopped unread sessions, working sessions,
  then the remaining ready and inactive sessions. Existing project/model grouping and pinned
  placement remain available. Existing list scopes and paging still determine which sessions appear.
- A working descendant keeps its session family working; descendant completion alone does not
  create unread status for the root. Blocking descendants retain needs-input priority.
- Thread responses expose `unread` independently of runtime `status`.
- `thread/unread/set` takes `threadId` and `unread`, returning the authoritative unread flag.
  Setting false during an active turn returns the unchanged flag.
- `thread/unread/changed` reports `threadId` and `unread` to observing clients.
- `codex agents --json [--watch]` exposes root `unread`. Its exclusive `status` can additionally
  be `unread`, between `needsInput` and `working` in display priority. Its `attention` includes
  `unread` only when the session belongs to the stopped unread group.
- A notification watcher may use these fields and explicitly acknowledge an activated session.
  No notification watcher is installed by this feature.

## Validation expectations

- Accepted prompts arm unread before their request succeeds; rejected prompts do not arm it.
- Running turns and approval/input waits reject acknowledgement without deferring it.
- Successful completion and interruption retain unread; acknowledgement clears it once stopped,
  and a later prompt or `/unread` restores it.
- Reopening the server, reading full or partial history, listing through either storage path,
  and reconciling historical metadata preserve acknowledgement.
- Passive reads and watch streams never acknowledge sessions. Watch updates reconcile unread
  notifications received during in-flight reads.
- Snapshots cover unread list rows and filters, priority against working and blocked sessions,
  the bright blue stopped-session footer indicator, and the JSON output. Slash commands never
  submit agent input. Focus tests cover the one-second delay, cancellation, and the absence of
  automatic retries after a working-session acknowledgement is ignored.
