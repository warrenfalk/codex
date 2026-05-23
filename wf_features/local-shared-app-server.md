# Local Shared App Server

## What we're trying to do

We want two different "connected" stories.

`Remote` mode means the TUI is talking to an app-server that should be treated
as authoritative for environment-sensitive behavior. In that mode, the TUI
should act like a remote client: remote cwd semantics, no local fallback, and
connection failures should behave like failures.

`LocalShared` mode means the TUI is still fundamentally local, but it is using a
shared app-server process instead of starting a private embedded one. This is
for the case where a user wants the same local behavior as standalone Codex
while still sharing backend state through a local app-server.

The key product goal is:

- `Embedded`: private local backend, standalone behavior.
- `LocalShared`: shared local backend, but still standalone-style local
  semantics.
- `Remote`: shared remote backend, connected-mode remote semantics.

The reason to separate `LocalShared` from `Remote` explicitly is that transport
alone is not enough to tell us what the user intended. A websocket endpoint can
look local because of SSH port forwarding, and a local shared backend should not
inherit the UX and cwd assumptions of a true remote connection.

## Final behavior

- The unpublished config key `tui.app_server_url` is repurposed and renamed to
  `tui.local_app_server_url`.
- `tui.local_app_server_url` behaves like `--local`: it attempts to connect to a
  shared local app-server, but otherwise the TUI behaves like standalone Codex.
- `--remote` selects true remote semantics.
- `--remote` wins over `--local` and over `tui.local_app_server_url`.
- `--remote-auth-token-env` is only valid with `--remote`.
- `LocalShared` is the only connected mode that may fall back to an embedded
  app-server.
- That fallback only happens when the initial connection to the configured local
  shared backend fails.
- When `LocalShared` falls back, the session is degraded and the footer shows a
  red `[L]`.
- `Remote` has no embedded fallback. If the remote app-server cannot be reached,
  startup fails.
- `Embedded` and `LocalShared` both use local cwd semantics for session lookup
  and resume flows.
- That includes the resume picker, `--last`-style latest-session lookup, and
  other session-selection paths that filter by cwd.
- `Remote` does not apply an implicit local cwd filter. It only uses a cwd when
  the remote session has an explicit remote cwd override.
- When the TUI is connected to any shared app-server, the footer still uses the
  connected badge `[R]`.
- If a shared app-server disconnects after startup, the footer shows the red
  disconnected `[R]` state while reconnect logic runs.

## Why this shape

This fork needs to survive repeated rebases on top of upstream changes, so the
implementation should avoid pushing a large new mode enum through every layer of
the TUI.

The design goal is to keep most existing "connected mode" code intact while
teaching startup and session handling one extra distinction: whether a shared
backend should use remote semantics or local standalone semantics.

That is why the implementation keeps most of the existing connected-mode
plumbing and concentrates the semantic split in startup target selection and
`AppServerSession` thread-parameter mode instead of rewriting the app around a
broad three-way mode split.

## Implementation details

### Startup selection

- CLI adds `--local`, parallel to `--remote`.
- Config adds `tui.local_app_server_url`.
- Startup resolves one of three requested modes:
  - `Embedded`
  - `LocalShared`
  - `Remote`
- `LocalShared` attempts a connection to the supplied local endpoint without
  remote-auth semantics.
- `Remote` attempts a connection to the supplied remote endpoint with the
  existing remote handling.

### Target shaping

The TUI startup path uses:

- `RequestedAppServerMode` for CLI/config intent.
- `AppServerTarget::LocalDaemon { endpoint }` for shared local app-server
  connections.
- `AppServerTarget::Remote { endpoint }` for true remote app-server
  connections.

The important detail is that "shared endpoint transport" and "remote semantics"
are no longer treated as the same thing.

### Session semantics

`AppServerSession` now carries the effective thread-parameter mode explicitly.
That matters because reconnects and session-picker restarts rebuild sessions
from a shared client, and transport type alone would incorrectly reclassify
`LocalShared` as `Remote`.

The rule is:

- a shared endpoint can still use local semantics when it came from `--local` or
  `tui.local_app_server_url`
- reconnect, resume, and picker-restart paths must preserve the original
  semantics
- transport alone must not decide whether cwd-sensitive session filtering is
  local or remote

### Cwd and config behavior

The TUI only suppresses local cwd/config behavior when it is using remote
semantics.

That means `LocalShared` keeps the same local behavior as standalone Codex for
things such as:

- cwd-derived config loading
- cwd prompts
- local session filtering and resume behavior
- resume-picker filtering based on the active local cwd
- latest-session lookup using the same cwd rules as standalone Codex
- other flows that should only switch behavior for true remote sessions

`Remote` continues to omit local cwd overrides and treats the remote app-server
session as authoritative.

### Resume picker behavior

The earlier cwd-related resume-picker fixes belong to this feature story once
`LocalShared` exists.

The desired product rule is not "shared endpoint transport means remote picker
behavior." The rule is:

- `Embedded`: picker and latest-session lookup use local cwd semantics
- `LocalShared`: picker and latest-session lookup also use local cwd semantics
- `Remote`: picker and latest-session lookup use remote semantics and only apply
  cwd filtering when an explicit remote cwd override exists

That keeps `LocalShared` aligned with the user promise of the feature: shared
backend process, but local standalone behavior.

### Footer semantics

Footer behavior remains intentionally small:

- connected shared backend: `[R]`
- disconnected shared backend: red `[R]`
- `LocalShared` startup fallback to embedded: red `[L]`

This keeps the existing connected/disconnected UI mostly intact while making the
degraded local fallback state explicit.

### Conflict minimization

To reduce future rebase conflicts, the implementation deliberately keeps broad
internal naming and wiring stable where possible. For example, existing
"connected/shared app-server" fields remain in place even when they now cover
both `LocalShared` and `Remote`. The semantic split is concentrated in startup
resolution, target shaping, and `AppServerSession`, which is the smallest place
to express the behavior difference without spreading a large enum through the
whole app.
