# Session Persistence Retries

## What it adds

This feature makes rollout persistence resilient to transient write failures
instead of treating persistence as a single fragile write path.

## Final behavior

- User-visible session progress should still be delivered even if writing the
  rollout file temporarily fails.
- Rollout persistence should enter a degraded mode when writes fail, keep the
  unwritten items in memory, and retry automatically with backoff.
- Once persistence recovers, queued rollout items should be written in order.
  Thread state metadata remains ordered by the current LiveThread metadata-sync
  path instead of a recorder-owned SQLite sync worker.
- Flush or shutdown should force a retry rather than silently dropping pending
  rollout data.
- Failed partial writes should be rolled back so the JSONL rollout file stays
  structurally valid.
- The in-memory backlog should stay bounded so a long-lived persistence failure
  cannot grow without limit.
- The backlog budget must not reject a valid large session item or compaction
  checkpoint on healthy storage. Checkpoints containing retained images can
  exceed 16 MiB. Flush older queued data before accepting a batch that would
  exceed the normal backlog budget. One oversized batch may remain pending;
  further batches must wait for successful drainage rather than accumulating
  behind it during an outage.
- Compaction must not report success until its replacement-history checkpoint
  has been accepted and flushed. If acceptance fails, keep the previous live
  history. If flushing fails after acceptance, retain the checkpoint for retry
  and keep live history consistent with that pending checkpoint. In either case,
  surface a persistence error rather than a successful compaction notification.
  If optional post-turn compaction fails to save, keep the completed answer and
  show a warning about the persistence failure.
- Resuming after a successful compaction must restore the saved checkpoint,
  including retained images, without reviving pre-compaction tool output or
  assistant messages. Validate this with a checkpoint larger than 16 MiB and
  with simulated write failures followed by recovery.

## Why it matters

Session recording is part of the product, not just a logging detail. A
temporary filesystem error should not make Codex lose the thread or stop
capturing the session once storage becomes healthy again.

Original implementation commit: `65d8e575fa` (`make session persistence retry on failure`)
