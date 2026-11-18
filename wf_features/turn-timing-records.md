# Durable turn timing observations

Session histories retain client-side timing metadata for turn waterfalls without correlating
rotating application logs or enabling full request/response payload capture. Recording is enabled by default for
normal model-backed turns. Existing histories without timing records remain readable.

## Observable behavior

- Session JSONL includes model-invisible `timing_record` records. Their versioned payloads identify the thread,
  turn, logical sampling request, stream-recovery attempt, span, parent span, and observed phase.
- A fresh logical request has its own ID. Stream recovery preserves that request ID and increments the attempt
  number. Client transport operations have distinct span IDs, including operations made during authentication
  recovery or transport fallback. Initial request preparation can carry attempt number zero.
- Started/completed span pairs cover request preparation, sampling attempts, client model-request scopes,
  transport attempts, stream setup, model-catalog refreshes, tool waits, and retry recovery.
- A client model-request scope includes local setup, hooks, stream acquisition, and stream consumption. Stream
  setup is separately measured. These boundaries do not assert when the backend accepted or began computing a
  request. Transport-attempt spans likewise describe client operation lifetimes, not packet-send timestamps.
  SDK-internal HTTP retries can occur within one transport-operation span.
- The model-request span ends before post-response hooks and tool-result waits. Tools may already be running
  while a model request remains in flight. Tool-wait spans represent the harness waiting for outstanding results,
  rather than the sum of individual command durations.
- Response-created observations retain server response IDs. Completed request/transport records retain response
  IDs when known, allowing joins with existing usage records, including responses without token-usage data.
- Stream item records retain added/done delivery times, item IDs, and item types for reasoning, tool calls, and
  messages. Multiple reasoning items can belong to one response and never imply additional generation requests.
- Stream summaries retain event and delta counts, delta byte counts, first/last observed event and delta times,
  accumulated waits for the next harness stream event, and accumulated event-processing time. These waits include
  channel-buffering effects; they are not direct measurements of server compute or wire latency.
- Item summaries retain delta counts and first/last delta times without retaining delta content. Deltas without
  item identity remain request-wide when overlapping items make attribution ambiguous. A bounded limit
  on concurrently open item summaries prevents unbounded metadata growth; omitted starts, including items without
  a stable identity, are counted explicitly.
  Repeated token deltas do not generate repeated persisted timing records.
- Completed, failed, and cancelled operations have distinct terminal statuses. An interrupted consumer closes
  outstanding transport observations as cancelled, even if producer cleanup completes later. Duplicate terminal
  callbacks cannot produce duplicate completion records for the same span.
- An item with no observed added event has a done observation only. A process crash or forced task destruction
  can leave a started span without a persisted end. Consumers must preserve these uncertainties rather than
  invent successful completions or reasoning durations.

## Data and compatibility boundaries

Timing records contain timestamps, IDs, requested model names, bounded counters, and lifecycle metadata. They do
not add request bodies, tool input/output text, reasoning text, encrypted reasoning content, credentials, headers,
or exception strings to the timing data.

Event timestamps are Unix milliseconds captured at observation time; persistence timestamps can be later.
Durations use a monotonic clock and are retained in microseconds. Wall-clock adjustments must not change measured
durations. Spans can overlap; consumers must use interval unions for wall-time allocation.

Recording does not change model-visible history, prompt construction, tool execution, retries, budgets, token
accounting, or cancellation decisions. Timing-only batches do not update thread recency, create pending metadata
touches, or release deferred metadata updates. Timing records survive resume and remain excluded from model context,
memory-extraction input, child-agent inherited context, chat history projections, and session search text. Rollout
migration excludes timing records associated with historically rolled-back turns. Consumers of the JSONL format
must recognize or ignore the new record type. Timing remains separate from live chat items;
dedicated historical/live app-server access is described in `app-server-turn-timing.md`.

Reasoning added/done times describe the lifecycle exposed by the provider. If a provider delivers an already
produced reasoning chunk with adjacent added/done events, the harness cannot recover the chunk's actual compute
start. Backend queueing, computation, buffering, and network latency remain distinct from client observations.
Historical sessions cannot acquire timing evidence retroactively.

Coverage follows the normal sampling loop. Standalone compaction, connection prewarming, and other auxiliary
requests can remain outside these spans; consumers must keep uncovered time visible.

## Validation expectations

- A response with multiple reasoning items produces one correlated model-request lifetime, with separately
  identified item deliveries and no extra API request caused by the timing recorder.
- A tool followed by another sampling request yields separate request IDs, and the initial request ends before
  the tool-wait interval ends.
- Resume round-trips the records and preserves the absence of timing data in subsequent inference input.
- An early stream close followed by recovery retains one logical request, different attempts, failure/success
  statuses, and an observed recovery interval.
- Interrupting an outstanding response records cancellation rather than a successful completion.
- Duplicate callbacks are idempotent; missing item starts remain missing; large delta sequences update counters
  with a bounded number of persisted records.
- Compact timing capture works when full trace-bundle capture is disabled, without retaining model payloads.
- Gated metadata persistence does not delay first sampling or turn completion because of timing-only batches.
