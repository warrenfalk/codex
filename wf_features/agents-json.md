# Agents dashboard JSON snapshots

`codex agents --json` prints one compact JSON object and a newline, flushes stdout,
and exits. `codex agents --json --watch` prints an initial snapshot and complete
replacement snapshots when the represented state changes. `--watch` requires
`--json`. Plain `codex agents` keeps the interactive dashboard.

The command observes an existing app-server without starting a daemon, creating or
resuming a session, attaching to threads, answering requests, or keeping idle
threads loaded. `--local`, `--remote`, and `--remote-auth-token-env` retain their
existing syntax and can appear before or after `agents`. Endpoint precedence is
remote, explicit local, `tui.local_app_server_url`, then the default daemon socket.
Profile and configuration errors are fatal. Project directories do not filter the
list. Remote paths are server paths and are never inspected as local Git projects.

## Membership

A session is a root task and its descendants, counted once using the root thread's
opaque ID. Include every family with a loaded root or descendant, plus the 20 most
recent non-archived top-level tasks, using the dashboard's interactive and
exec/app-server sources and recency ordering. Ephemeral threads are excluded.
Discover unloaded ancestors when necessary; never present a child as its own root.

Watch mode retains discovered tasks after they unload, as the dashboard does.
If a thread unloads during a metadata or history read, keep its last known metadata,
mark it unloaded, and continue watching on the same connection. Metadata received
in recent-task listings or thread-start notifications also counts as known state.
If only an ID was discovered before the thread became unreadable, skip that ID
without inventing a row; later discovery or notifications can make it visible.
Archive and deletion remove tasks. Reconnection refreshes retained tasks and
reconciles archives/deletions that happened during the interruption, then adds the
current loaded and recent tasks. All discovery queries exhaust the pages needed
for their result; the recent seed continues past descendant-only pages. Events
received during reads are reconciled before emitting the completed snapshot.

## Version 1 JSONL contract

Every line has `version: 1`, `connection`, `counts`, and `sessions`.

When connected, `sessions` is an array sorted lexicographically by opaque root ID.
`counts.total` is its length, `counts.working` counts records whose `working` is
true, and `counts.needsAttention` counts records with nonempty `attention`.

Each session contains:

| Field | Meaning |
| --- | --- |
| `id` | Root thread ID, suitable for opening the task. |
| `title` | Dashboard display title: first trimmed line of name, otherwise preview, otherwise `Untitled task`. |
| `name`, `preview` | Nullable saved name and the dashboard's full searchable preview, including its latest-user-message fallback. |
| `cwd` | Absolute root working directory. |
| `createdAt`, `updatedAt`, `recencyAt` | Root Unix timestamps in seconds; recency can be null. |
| `gitInfo` | Nullable recorded Git metadata, including branch. |
| `project` | Dashboard group `key` (two strings) and display `heading`. Local worktree grouping follows the feature setting; otherwise the key is `[cwd, ""]` and heading is cwd. |
| `status` | Exclusive dashboard group: `needsInput`, `working`, `ready`, or `finished`, in that priority order across the family. |
| `rootStatus` | Root runtime status object, including active flags; distinguishes the root's state from descendant activity. |
| `loaded` | At least one family member is loaded. |
| `working` | At least one member is active without approval or user-input blockers. |
| `attention` | Unique reasons across the family, in order: `approval`, `userInput`, `error`. |

Working and attention are independent: one working child and one blocked child
contribute to both counts. `ready` means there is an idle member with no higher
priority status; `finished` means every observed member is unloaded.

These fields reproduce the standard top-level list, including title/search,
project headings and counts, status labels and symbols, and ordering. To reproduce
status grouping, order by status priority, descending `updatedAt`, then ascending
ID. For project grouping, stably sort that order by project key and descending
`updatedAt`. Selection, search input, the current-task marker, and the composer
are local interactive view state. The selected task's live detail panel is outside
this top-level list contract.

## Delivery and failures

Emit changes to any represented field, including membership, name/preview,
directory/group, timestamps, activity, attention, and connection state. Suppress
identical consecutive snapshots. Each line is flushed immediately. Stdout contains
only JSONL; diagnostics use stderr. Consumers should tolerate additional fields.

On transient connection loss or lost synchronization, watch emits
`{"version":1,"connection":"disconnected","counts":null,"sessions":null}`.
It retries with exponential backoff from 0.5 seconds up to 30 seconds. A successful
full synchronization resets backoff and produces a fresh connected snapshot.
Disconnected never means zero tasks; a connected empty array does.

One-shot failures exit nonzero without a successful snapshot. Fatal endpoint,
configuration, authentication, or incompatible-protocol errors exit nonzero in
either mode. Ctrl+C stops observation. Closing the consumer's pipe stops output.

Validation must cover complete discovery and ancestry, notifications racing with
initial reads, simultaneous work and attention, recent/retained membership,
archive/delete and reconnect reconciliation, stable JSON snapshots, flushing,
duplicate suppression, clean one-shot failure, fatal authentication, endpoint
argument placement, unload races during metadata/history reads and subsequent
reloads, and unchanged interactive defaults.
