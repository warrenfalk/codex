## Connected TUI Current State

This document reflects the currently inspected rebased tree, not just prior implementation context.
The relevant code paths are primarily in:

- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/connected_app_server.rs`
- `codex-rs/tui/src/resume_picker.rs`
- `codex-rs/tui/src/remote_sessions.rs`
- `codex-rs/tui/src/sessions_picker.rs`
- `codex-rs/cli/src/main.rs`

### Goal

Connected mode allows the existing TUI to run against a detached `codex app-server` over websocket
JSON-RPC instead of always using the embedded in-process app-server/core path.

The design is still intentionally incremental:

- preserve the existing TUI as much as possible
- bridge remote app-server traffic back into the TUI's existing internal event handling
- avoid a full rewrite onto the app-server v2 model all at once

### Current User-Facing Entry Points

- `codex --connect [WS_URL]`
- `codex-tui --connect [WS_URL]`
- `codex resume ... --connect [WS_URL]`
- `codex fork ... --connect [WS_URL]`
- `codex sessions --connect [WS_URL]`

`--connect` defaults to `ws://127.0.0.1:4222` when provided without a URL.

### What Currently Works

- Starting a fresh connected session over websocket.
- Sending normal user turns with streaming output.
- Translating app-server notifications back into the existing TUI event flow.
- Remote resume by thread id.
- Remote `resume --last`.
- Remote fork by thread id.
- Remote `fork --last`.
- Startup remote resume/fork picking via the connected picker.
- In-session connected thread replacement for:
  - new session
  - resume
  - fork
- Exec approval round-trips.
- Patch approval round-trips.
- `request_user_input` round-trips.
- Remote picker search now uses server-side `thread/list.searchTerm`.
- A separate active-sessions flow exists via `codex sessions --connect ...`, backed by:
  - `thread/loaded/list`
  - `thread/read`
  - live updates from app-server notifications
- `codex sessions` currently focuses the selected remote thread in kitty.

### Current Architecture

There are now two distinct remote helpers:

1. `connected_app_server.rs`

- This is the main connected-TUI session transport.
- It owns the live websocket for a connected chat session.
- It starts/resumes/forks threads with:
  - `thread/start`
  - `thread/resume`
  - `thread/fork`
- It sends user operations with `turn/start` and `turn/interrupt`.
- It translates server notifications and some legacy `codex/event/*` notifications back into
  internal `EventMsg` values the TUI already knows how to render.
- It also bridges server requests for approvals and `request_user_input` back into TUI responses.

2. `remote_sessions.rs`

- This is a smaller client used only for the `codex sessions` flow.
- It is not the main conversation transport.
- It provides:
  - `thread/loaded/list`
  - `thread/read`
  - notification consumption for refreshing the active sessions picker

### Important Current Behavior

- Connected session shutdown now attempts `thread/unsubscribe` before tearing down the websocket.
- Connected picker search is remote-aware: changing the query resets pagination and requests
  `thread/list` with `searchTerm`.
- Connected mode still depends on a mix of:
  - v2 app-server notifications/requests
  - legacy `codex/event/*` compatibility notifications
- For approval-like interactions, the connected client intentionally ignores the legacy duplicate
  event forms and uses the JSON-RPC request path, because only that path carries the response id.

### Confirmed Current Limitations

- Unsupported connected operations still fall through to the generic warning:
  `connected mode POC ignores this action`
- The op bridge still explicitly no-ops or ignores several operations, including:
  - `Op::AddToHistory`
  - `Op::ListCustomPrompts`
  - `Op::ListSkills`
  - `Op::OverrideTurnContext`
  - `Op::ReloadUserConfig`
  - any other unmapped `Op`
- Connected mode still synthesizes TUI state from server data rather than being natively modeled on
  the full app-server v2 client shape.
- `codex sessions` is currently focused on active loaded sessions only, not a full general session
  browser.

### TODOs

- `connected mode POC ignores this action` on `/rename`
- Losing the websocket connection exits immediately with an error instead of temporarily showing
  some kind of "disconnected" status. While disconnected, it should allow exiting manually.

### Notes On The Two TODOs

#### `/rename`

The TUI still emits `Op::SetThreadName { name }` from the rename flow, but the connected transport
does not currently map that operation to an app-server RPC. In `connected_app_server.rs`, unmapped
ops fall into the generic:

- `connected mode POC ignores this action`

So `/rename` is currently confirmed broken in connected mode.

#### Disconnect Handling

The connected reader/writer tasks still emit `AppEvent::FatalExitRequest(...)` on websocket read or
write failure, including the plain close case:

- `app-server websocket connection closed`

`AppEvent::FatalExitRequest` is handled in `app.rs` by immediately exiting with
`ExitReason::Fatal(...)`. There is no intermediate disconnected state, retry state, or
"connection lost, press q/esc/ctrl-c to exit" UI.

### Rebasing / Upstream-Integration Notes

Compared with the earlier pre-rebase mental model, the current tree has at least these notable
upstream-influenced differences:

- service tier and approvals reviewer fields are now carried through connected
  `thread/start` / `thread/resume` / `thread/fork`
- the local session picker uses `SessionTarget { path, thread_id }` instead of only a path
- there is now a dedicated `codex sessions` command and supporting TUI
- connected session shutdown now explicitly unsubscribes before cancellation

### Practical State Assessment

Connected mode is past the original "simple prompt POC" stage. It now supports a useful detached
workflow for:

- fresh sessions
- resume
- fork
- approvals
- server-side session search

The biggest remaining rough edges are no longer basic prompt handling. They are:

- unmapped interactive actions such as rename
- brittle disconnect behavior
- continued reliance on compatibility bridging rather than a fully native app-server v2 client
