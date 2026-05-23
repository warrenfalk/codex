# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.

## TUI app server

Interactive TUI sessions can default to a shared local app-server endpoint by
setting `tui.local_app_server_url` in `~/.codex/config.toml`:

```toml
[tui]
local_app_server_url = "ws://127.0.0.1:4500"
```

When this setting is present, Codex first tries the configured shared local
backend. If it cannot connect at startup, it falls back to a standalone private
embedded app-server session and shows a red `[L]` badge in the status line.
