# SessionHeaderHistoryCell

Code paths: `new_session_info`, placeholder session header, inferred child-session state.

| Render field | Source | Transform |
|---|---|---|
| `version` | Not `ServerNotification`-sourced: local `CODEX_CLI_VERSION` / redraw override. | Rendered in the title line. |
| `model` | Primary sessions: non-notification start/resume/fork response converted into `ThreadSessionState`; inferred child sessions clone local/default model state. | Rendered on the model row. |
| `model_style` | Not `ServerNotification`-sourced: local placeholder/current-session styling. | Applied to the model text and `fast` label. |
| `reasoning_effort` | Primary sessions: non-notification session response state; otherwise local/default config. | Optional reasoning suffix on model row. |
| `show_fast_status` | Not `ServerNotification`-sourced: local feature/config/session state. | Adds `fast` label on the model row. |
| `directory` | Primary sessions: non-notification session response `ThreadSessionState.cwd`; inferred child session can use `ServerNotification::ThreadStarted.thread.cwd`. | Home-relative and center-truncated. |
| `yolo_mode` | Not directly `ServerNotification`-sourced: session approval policy and permission profile, or local config for placeholder/redraw. | Adds the `permissions: YOLO mode` row. |
