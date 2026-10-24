# Active View Terminal Titles

## What it adds

This feature turns terminal and window titles into an active part of the TUI so
the title reflects the current view and identifies the current session clearly.

## Final behavior

- The terminal title is no longer effectively static; it is recomputed from the
  active view.
- Title segments can include project, branch, thread name, spinner, runtime
  status, model, and task progress.
- Branch lookup state is preserved even when the status line itself is not using
  the branch, because the terminal title may still need it.
- When a thread has a title, the window title format foregrounds it as
  `Codex <repo>: "<thread title>"`, with the branch appended in parentheses when
  available.
- If the thread is unnamed, the title falls back to project-only context.
- After running an external program, the TUI restores the title it was managing
  before control returned.
- Redundant title writes are avoided so the UI does not flicker unnecessarily.

## Why it matters

The terminal title becomes a compact live summary of the session, and the
session-aware formatting makes multiple Codex windows from the same repository
much easier to distinguish in multiplexers, tab bars, and window switchers.

Original implementation commit:
- `a7a3ca2e45` (`tui: set terminal titles from the active view`)
- `c593d6ff51` (`tui: make window titles identify the active session`)
