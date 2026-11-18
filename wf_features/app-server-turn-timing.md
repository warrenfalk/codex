# Historical and live turn timing access

Clients can inspect client-side timing for any persisted thread and observe a running turn without
starting inference, resuming the thread, acquiring control, or subscribing to ordinary conversation
events. Timing access is separate from chat history and model-visible input.

## Observable behavior

- `thread/timing/read` accepts a thread ID, an optional turn ID, a cursor and a page limit. It returns
  ordered, content-free timing observations with stable entry IDs, an opaque resume cursor, an
  optional next-page cursor, observation time, and a count of skipped malformed/unsupported records.
- Omitted turn ID selects all timing observations in the thread's selected history. Supplied turn
  ID stays fixed while the thread gains further turns. Records retain their original thread,
  request, attempt, span, parent-span and response identities, including inherited history.
- Read and subscription payloads use camelCase. Timestamps retain Unix millisecond precision and
  measured durations/counters retain microseconds. Durable JSONL keeps its existing format.
- Limits are bounded to 1–1000 observations, default 500. Scans also have a byte budget, so an empty
  page with a next-page cursor is valid. A single complete JSONL record remains indivisible.
- A resume cursor remains useful at EOF: later reads return newly appended observations. Only
  complete newline-terminated records advance it; a partial trailing record is retried later.
  Cursors cannot cross threads or turn filters. A changed selected rollout/history invalidates
  the cursor explicitly rather than silently skipping or mixing observations.
- Historical reads support legacy and paginated rollouts, archived/compressed history, and bounded
  inherited rollout prefixes. Reads do not create an untouched thread's lazy rollout file or
  change its archive status, recency, unread state, input queue, or interactive subscriptions.
- `thread/timing/subscribe` accepts a client-chosen, connection-local subscription ID, thread ID,
  optional turn ID and optional read/update cursor. Ordered `thread/timing/updated` notifications
  backfill from that cursor and continue with newly available observations. The same stream carries
  initial and later pages, eliminating a separate snapshot/subscription race. Notifications may
  arrive before the subscribe response and always identify their subscription.
- Live updates include provisional open spans, monotonic elapsed time and available stream counters,
  including a currently pending receive wait or event handler. Progress is sampled at most four
  times per second after backfill. Counters can advance without new output, and reading them never
  creates persisted per-token/progress records or additional API generation requests.
- Active snapshots are process-local and separate from durable entries. A missing active span is
  not a successful completion. Persisted completed/failed/cancelled observations remain authoritative;
  a crash can leave an unfinished span. Older turns do not gain missing evidence retroactively.
- Subscription failures emit `thread/timing/subscriptionClosed` with an explicit error. Clients may
  retry from their last received cursor, or restart a changed history without a cursor. Entry IDs
  support deduplication when replaying a previously received page.
- `thread/timing/unsubscribe` is idempotent and connection-scoped. Its acknowledgement fences all
  subsequent updates for that subscription. Disconnects and server shutdown release subscriptions.
  Duplicate subscription IDs on one connection are rejected; each connection supports at most 16.
- Other storage backends may explicitly report unsupported timing reads. Timing does not become an
  ordinary chat item, memory-extraction input, search text, notification of a new answer, or input
  to the model. Tool command/output content is obtained through existing item/history APIs.

## Interpretation

These are client observations, including setup, stream waiting/processing and tool waits. They do
not identify backend reasoning-compute starts or separate backend queueing from buffering/network
latency. Added/done item delivery boundaries retain the existing recorder's meaning. Overlapping
spans must be combined with interval unions, not summed as elapsed wall time.

## Validation expectations

- Historical paging and reconnect replay preserve stable identities, timing precision and correlation
  while excluding unrelated payload fields and leaving thread attachment/model request counts unchanged.
- A gated model stream exposes increasing live receive-wait time before its final response, followed
  by the correlated durable completion. Cancellation closes spans without inventing success.
- Empty backfill pages, truncated trailing lines, malformed/unsupported timing data, archived and
  compressed rollouts, fork cutoffs and cursor/history changes have explicit tested behavior.
- Subscription IDs are isolated by connection, unsubscribing fences updates, duplicate IDs fail,
  and disconnect cleans up active subscriptions. No TUI or GUI controls are required for the API.
