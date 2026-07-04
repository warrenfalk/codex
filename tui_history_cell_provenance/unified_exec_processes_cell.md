# UnifiedExecProcessesCell

Code paths: local `/ps`, unified-exec process tracking.

| Render field | Source | Transform |
|---|---|---|
| `processes` | Not directly `ServerNotification`-sourced: local `/ps` snapshots `self.unified_exec_processes`. That state is seeded/updated by the sources below. | Renders one process row per tracked background terminal, or a fixed empty-state row when no entries exist. |
| `processes[].command_display` | Local tracked process state seeded by `ServerNotification::ItemStarted.item.CommandExecution.{id, process_id, command, source}` when `source == UnifiedExecStartup`. | Key is `process_id.unwrap_or(id)`; command is split and shell-wrapper stripped. |
| `processes[].recent_chunks` | `ServerNotification::CommandExecutionOutputDelta.{item_id, delta}` for tracked unified-exec processes. | Matched by call id, decoded lossily as UTF-8, line-trimmed, empty lines ignored, last three chunks retained. |
| process removal / empty state | `ServerNotification::ItemCompleted.item.CommandExecution.{id, process_id, source}`. | Unified-exec completion removes the tracked process; later `/ps` output can render the empty state. |
