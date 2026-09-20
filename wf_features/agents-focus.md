# Focus an existing agents TUI

`codex agents --focus SESSION_ID` focuses a running local TUI attached to the
selected session. It is silent on success and exits zero. It can be called from
Waybar or another process without a terminal, using the opaque root session IDs
from `codex agents --json --watch`.

The command never starts a terminal or TUI, starts or resumes a session, switches
the session displayed by a TUI, or posts a notification. It does not connect to or
start an app-server. `--focus` conflicts with `--json` and `--watch`; the existing
interactive dashboard and JSON modes keep their behavior.

`--local`, `--remote`, and `--remote-auth-token-env` use the usual endpoint syntax
and may appear before or after `agents`. Endpoint precedence is remote, explicit
local, `tui.local_app_server_url`, then the default daemon socket. Profile and
configuration validation still apply. Only local TUIs attached to that endpoint
qualify, regardless of their project directory or Codex home.

TUIs automatically become discoverable while running. Matching uses the current
primary session, including when viewing one of its subagents, and follows session
switches immediately. Retained tasks and background subscriptions do not qualify
as attached sessions. A TUI displaying a descendant also qualifies for known
ancestors in that family. Merely listing a session in a dashboard does not qualify.
A disconnected TUI can still be focused if it displays the matching session.
Window titles and process launch arguments have no role in matching.

Focusing currently requires Kitty on Unix, `kitten` on the TUI's PATH, a valid
Kitty window ID in the TUI's environment, and permitted Kitty remote control.
The operation selects that Kitty pane and its containing tab/window. It uses the
target TUI's terminal context; the caller needs no Kitty environment. Password
discovery and approval prompts are disabled. TUIs inside tmux or screen are
unsupported because selecting the outer Kitty pane cannot select their inner pane.

Among matching local TUIs that support focusing, choose the lowest process ID.
Revalidate the session immediately before focusing. A session switch between
discovery and focusing may cause another matching TUI to be selected. After an
actual focus operation fails or times out, report failure without trying another
TUI. No match, unsupported terminal/platform, remote-control rejection, missing
`kitten`, and unresponsive TUIs produce nonzero exits and useful stderr diagnostics.
There is never a launch or notification fallback. Older TUIs must be restarted
with a version that supports this command to be discoverable.

Validation covers endpoint/session matching, switching between sessions while old
tasks remain retained, deterministic selection, stale registrations, a switch
between discovery and focus, unavailable and failing backends, bounded waits,
cancelled requests, cleanup on TUI exit, and invocation without a terminal.
