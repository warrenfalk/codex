# TooltipHistoryCell

Code paths: `new_session_info`, `tooltips::get_tooltip`.

| Render field | Source | Transform |
|---|---|---|
| `tip` | Not `ServerNotification`-sourced: explicit tooltip override or local `tooltips::get_tooltip(auth_plan, show_fast_status)`. | Prefixes `**Tip:**`, markdown-renders, and wraps. |
| `cwd` | Usually not `ServerNotification`-sourced: `config.cwd`; inferred child session cwd can originate from `ServerNotification::ThreadStarted.thread.cwd`. | Used by markdown local-link rendering. |
