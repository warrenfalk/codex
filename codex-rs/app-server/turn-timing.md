# Turn timing API

Timing inspection is read-only and does not require `thread/resume`. These v2 methods are available
without the experimental API capability. They work independently of chat subscriptions and control
grants. Existing thread/turn/item methods supply conversation labels and tool lifetimes; timing
records contain no tool output or reasoning text.

## Read and backfill

```json
{"method":"thread/timing/read","id":1,"params":{"threadId":"THREAD_UUID","turnId":"TURN_ID","limit":500}}
```

`turnId`, `cursor`, and `limit` may be omitted or null. Omitting `turnId` selects all turns. Limits
are 1–1000. Each response contains:

- `data`: `{id, record}` entries. IDs are stable in the selected rollout; `record` contains the
  versioned timing metadata with camelCase fields and enum values.
- `cursor`: opaque checkpoint after the scanned prefix, also usable after EOF to read new data.
- `nextCursor`: continue immediately when non-null. A scan-byte limit can produce an empty page
  with a next cursor, so do not use `data.length` to decide whether backfill is complete.
- `active`: process-local open spans, each with its started `record`, `elapsedUs` and optional
  provisional `stream` counters. These are snapshots, not additional durable entries.
- `observedAtMs`: server Unix milliseconds for the snapshot; record `atMs` values retain actual
  observation times. `durationUs`, `elapsedUs`, `receiveWaitUs` and `eventProcessingUs` are microseconds.
- `skippedRecordCount`: malformed rollout lines or unsupported/malformed timing observations
  encountered during this scan. Absence of timing in an older turn is not proof of zero model wait.

Continue with the same thread/turn filter and `cursor: nextCursor`. Keep the final `cursor` for
refreshing or subscribing. A changed selected history returns an invalid-request error; discard
the old report/cursor and restart. Rollout paths are resolved by the server, never supplied by clients.

Initial backfill streams source records with bounded memory and a per-page scan budget. Plain live
rollouts seek directly to the checkpoint on subsequent reads. Compressed history uses the shared
read-only rollout reader; opening a compressed page may require decoding its source representation.

## Subscribe

```json
{"method":"thread/timing/subscribe","id":2,"params":{"subscriptionId":"timing-view","threadId":"THREAD_UUID","turnId":"TURN_ID","cursor":"LAST_READ_CURSOR"}}
```

The response is `{}`. Omit the cursor to receive history from its beginning. Updates use
`thread/timing/updated` with `{subscriptionId, threadId, turnId, page}`; `page` has the same shape
as a read response. Backfill and live updates share one ordered stream. Register the notification
handler before sending the request: initial notifications may precede its response. Keep the most
recent page cursor and deduplicate durable entries by ID if a reconnect replays a page.

After backfill, active spans/counters are sampled every 250 ms, only for active subscribers. A
receive-wait counter includes the wait currently in progress, so a quiet stream remains observable.
Snapshots do not persist progress records. An empty `active` array never proves successful
completion: use durable terminal records. Open bars can advance provisionally between updates.

Subscriptions remain open until explicitly removed or disconnected. A null/omitted turn filter can
follow later turns; a concrete turn filter stays pinned. A failed reader emits
`thread/timing/subscriptionClosed` with `{subscriptionId, threadId, error}` and stops. Reconnect by
subscribing with the last received cursor. Unsupported stores return method-not-found explicitly.

```json
{"method":"thread/timing/unsubscribe","id":3,"params":{"subscriptionId":"timing-view"}}
```

Unsubscribe returns `{}` and waits for its notification producer to stop. IDs belong to the
connection, may be reused after unsubscribe, and cannot cancel another connection's subscription.
There are at most 16 subscriptions per connection; IDs are 1–128 bytes.

These timings measure client activity, not backend reasoning compute or packet latency. Multiple
reasoning item deliveries can belong to one request. Preserve incomplete evidence and overlapping
model/tool spans when building a waterfall.
