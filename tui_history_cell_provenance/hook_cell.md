# HookCell

Code paths: `ServerNotification::HookStarted`, `ServerNotification::HookCompleted`, `on_hook_started`, `on_hook_completed`.

| Render field | Source | Transform |
|---|---|---|
| `runs[]` membership/order | `ServerNotification::HookStarted.run.id`; `ServerNotification::HookCompleted.run.id`. | `id` pairs begin/end events but is not displayed. Runs remain separate internally; adjacent visible running runs can be coalesced for display. |
| `runs[].event_name` | `ServerNotification::HookStarted.run.event_name`; updated from `ServerNotification::HookCompleted.run.event_name`. | Mapped by `hook_event_label`; rendered as `Running <event> hook` or `<event> hook (<status>)`. |
| `runs[].status_message` | `ServerNotification::HookStarted.run.status_message`; completion can refresh the same field but completed render does not display it. | Non-empty running status is appended after `: ` and participates in running-row grouping. |
| running-state `start_time` | Not `ServerNotification`-sourced: `Instant::now()` when the run enters the cell. | Drives spinner/shimmer timing and grouped running-hook indicator timing. |
| completed-state `status` | `ServerNotification::HookCompleted.run.status`. | Rendered lowercased in the completed hook header; controls bullet color and quiet-success persistence. |
| completed-state `entries[].kind` | `ServerNotification::HookCompleted.run.entries[].kind`. | Chooses output prefix (`warning:`, `error:`, `feedback:`, etc.); warning output changes a successful bullet from green to default bold. |
| completed-state `entries[].text` | `ServerNotification::HookCompleted.run.entries[].text`. | Split by newline and indented under the completed hook row. |
| reveal / linger deadlines | Not `ServerNotification`-sourced: local timers and constants. | New runs begin hidden briefly; visible quiet successes can linger before disappearing. |
| `animations_enabled` | Not `ServerNotification`-sourced: `self.config.animations`. | Controls activity indicator, shimmer/bold fallback, and animation tick scheduling. |

Non-render hook summary fields include handler type, execution mode, scope, source path/source, display order, started/completed timestamps, and duration.
