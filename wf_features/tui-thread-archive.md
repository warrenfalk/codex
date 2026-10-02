# TUI Thread Archive

## What it adds

The TUI can move a completed chat out of the normal resume list without
deleting it. Archived chats stay recoverable, but active session search remains
focused on work that is still current.

## Final behavior

- `/archive` archives the current chat and immediately opens a fresh chat.
- `/archive` is unavailable while a task is running.
- `/archive` is not available from side conversations.
- A chat must have a materialized thread before it can be archived.
- Archiving does not ask for a reason or final disposition.
- `/archive` rejects extra text instead of treating it as a reason.
- The resume picker has a toolbar scope control with `Active` and `Archived`
  values.
- The picker defaults to `Active`, which is the existing non-archived session
  list.
- Switching the scope to `Archived` reloads the picker from archived sessions
  and keeps the search box available for narrowing those results.
- Selecting an archived chat from the resume picker restores it to the active
  session store before resuming it.
- Forking an archived chat is allowed from the picker without restoring the
  original chat to the active list.

## Why it matters

Long-lived local Codex use accumulates many chats that are worth keeping but no
longer need to crowd the default resume view. Archive gives those chats a low
friction off-ramp while preserving a discoverable recovery path in the same
picker users already know.

## Validation expectations

- The slash-command popup lists `/archive` near the other session lifecycle
  commands.
- Running `/archive` on an idle materialized chat removes that chat from the
  active resume picker and starts a new chat.
- The resume picker toolbar can be focused with Tab and changed with left/right
  arrows to show archived sessions.
- Search still filters the currently selected scope.
- Selecting an archived session resumes it normally and it appears in the
  active resume list afterward.
