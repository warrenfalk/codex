# Connected Footer Badge

## What it adds

This feature makes connected-mode state visible in the footer instead of leaving
the user to infer it from startup flags or backend behavior.

## Final behavior

- When the TUI is attached to a shared app-server, the footer/status line shows
  an `[R]` badge.
- The badge is light blue while the backend connection is healthy.
- The badge turns red when the shared backend disconnects.
- The badge survives chat-widget replacement and thread/view transitions, so
  connected state does not disappear during normal TUI reconstruction.

## Why it matters

Connected mode changes how the session is being hosted. A persistent footer
badge gives the user a low-noise way to confirm that state at a glance and to
notice when the shared backend has dropped.

Original implementation commit: `a6ace70454` (`connected tui: add [R] to footer`)
