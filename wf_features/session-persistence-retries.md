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

## Why it matters

Session recording is part of the product, not just a logging detail. A
temporary filesystem error should not make Codex lose the thread or stop
capturing the session once storage becomes healthy again.

Original implementation commit: `65d8e575fa` (`make session persistence retry on failure`)
