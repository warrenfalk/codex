# ExecCell

Code paths: `ItemStarted`/`ItemCompleted` handling, `CommandExecutionOutputDelta`, `new_active_exec_command`.

| Render field | Source | Transform |
|---|---|---|
| `calls[]` membership/order | `ServerNotification::ItemStarted.item.CommandExecution.id`; fallback cells can be created from `ServerNotification::ItemCompleted.item.CommandExecution.id`. | `id` is used as a routing key, not displayed. Active compatible calls can be grouped into one exploring cell; mismatched completions become separate cells. |
| `calls[].command` | `ServerNotification::ItemStarted.item.CommandExecution.command`; fallback from `ServerNotification::ItemCompleted.item.CommandExecution.command` when no matching running command exists. | Split into argv-like command vector, shell wrapper stripped for display, then shell-highlighted/wrapped. |
| `calls[].parsed` | `ServerNotification::ItemStarted.item.CommandExecution.command_actions`; fallback from completed item `.command_actions`. | Converted from app-server command actions into core parsed commands; locally annotated for skill reads; drives exploring grouping and `Read`/`List`/`Search` labels. |
| `calls[].source` | `ServerNotification::ItemStarted.item.CommandExecution.source`; fallback from completed item `.source`. | Controls unified-exec filtering, title text (`You ran` vs `Ran`), user-shell output limits, and exploring eligibility. |
| `calls[].output.aggregated_output` | Live: `ServerNotification::CommandExecutionOutputDelta.delta`; final: `ServerNotification::ItemCompleted.item.CommandExecution.aggregated_output`. | Live deltas append to active output; completion replaces/finalizes output. Unified-exec interaction output is blanked for this cell. |
| `calls[].output.formatted_output` | `ServerNotification::ItemCompleted.item.CommandExecution.aggregated_output`. | Stored as formatted output for transcript rendering; live deltas only update `aggregated_output`. |
| `calls[].output.exit_code` | `ServerNotification::ItemCompleted.item.CommandExecution.exit_code`. | Missing value defaults to `0`; controls success/failure bullet and transcript result glyph. |
| `calls[].duration` | `ServerNotification::ItemCompleted.item.CommandExecution.duration_ms`. | Missing value defaults to `0`; converted to `Duration` and shown in transcript result lines. |
| `calls[].start_time` | Not `ServerNotification`-sourced: `Instant::now()` when the active call is created. | Drives active spinner timing; cleared when completion is applied. |
| `calls[].interaction_input` | Render-capable but not populated from `ServerNotification` in the current path; construction passes `None`. | If populated, would affect unified-exec interaction preview; actual terminal stdin is rendered by `UnifiedExecInteractionCell`. |
| `animations_enabled` | Not `ServerNotification`-sourced: `self.config.animations`. | Selects animated versus static active indicator. |
