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

## Edge cases

- A malformed `tui.local_app_server_url` should fail startup clearly instead of
  silently falling back.
- A well-formed but unreachable local shared endpoint should fall back only at
  startup. If the shared endpoint disconnects after startup, reconnect handling
  should run instead.
- `--remote-auth-token-env` should not be accepted for local shared mode.
- A local shared session must keep using local cwd filtering even when its
  endpoint is a websocket URL.
- A remote session must not accidentally inherit local cwd filtering from the
  client machine.

## Validation expectations

- Config parsing accepts `tui.local_app_server_url`.
- CLI parsing distinguishes `--local` from `--remote`.
- Startup tests cover local shared success, local shared fallback, remote
  failure, and precedence between CLI flags and config.
- Resume and latest-session lookup tests verify local cwd filtering for embedded
  and local shared sessions, and remote semantics for true remote sessions.
- Footer snapshots cover connected remote/local shared state and local fallback
  state.

Original implementation commit: `2fe924f44d` (`connected tui: local shared app server`)
