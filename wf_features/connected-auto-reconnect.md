# Connected Auto-Reconnect

## What it adds

This feature makes connected TUI sessions resilient to shared app-server
restarts and transient disconnects.

## Final behavior

- If the shared backend connection drops, the TUI enters an explicit reconnect
  state instead of treating the session as permanently dead.
- While reconnecting, outbound operations are disabled so the user cannot submit
  new work against a disconnected backend.
- Codex retries the shared backend connection automatically on a loop.
- If the original thread can be resumed, Codex reattaches to it and clears the
  reconnect status without re-rendering an extra session banner.
- If the server reports that the disconnected thread has no rollout or is not
  materialized yet, Codex starts a fresh shared session and tells the user what
  happened without automatically resending retained input.
- Generic resume failures do not create a new thread.

## Why it matters

Connected mode is much more usable when backend restarts are recoverable. This
change turns many disconnects into a temporary interruption rather than a lost
session.

Original implementation commit: `da2025fa31` (`connected tui: auto-reconnect sessions`)
