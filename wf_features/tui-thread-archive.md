# TUI Thread Archive

## What it adds

The TUI can move a completed chat out of the normal resume list without
deleting it. Archived chats stay recoverable, but active session search remains
focused on work that is still current.

## Final behavior

- `/archive` and `/archive new` archive the current chat and immediately open a
  fresh chat.
- `/archive exit` archives the current chat and exits the TUI without creating
  another chat, on embedded, local daemon, and remote app servers.
- Exiting after archiving reports that the session was archived instead of
  offering a normal resume command for it.
- `/archive` is unavailable while a task is running.
- `/archive` is not available from side conversations.
- A chat must have a materialized thread before it can be archived.
- Archiving does not ask for a reason or final disposition.
- The optional argument is exactly `new` or `exit`. Unknown arguments or extra
  text show `Usage: /archive [new|exit]` without archiving or exiting.
- Archiving must succeed before a new chat opens or the TUI exits. A failure
  keeps the current chat open and reports the error.
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
- Running `/archive` or `/archive new` on an idle materialized chat removes that
  chat from the active resume picker and starts a new chat.
- Running `/archive exit` archives that chat and exits without starting a new
  chat. Check this on embedded, daemon, and remote app servers.
- An archive failure leaves the current chat open for either argument.
- Invalid arguments, active tasks, and side conversations do not archive or
  exit the chat.
- The resume picker toolbar can be focused with Tab and changed with left/right
  arrows to show archived sessions.
- Search still filters the currently selected scope.
- Selecting an archived session resumes it normally and it appears in the
  active resume list afterward.
