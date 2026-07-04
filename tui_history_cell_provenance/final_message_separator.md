# FinalMessageSeparator

Code paths: `TurnCompleted` handling, task completion, streaming separator insertion.

| Render field | Source | Transform |
|---|---|---|
| `elapsed_seconds` | Usually `ServerNotification::TurnCompleted.turn.duration_ms`; fallback can be local status-widget elapsed time. | Milliseconds are converted to seconds; rendered only when greater than 60 seconds as `Worked for ...`. |
| `runtime_metrics` | Not `ServerNotification`-sourced: local `session_telemetry.runtime_metrics_summary()` accumulated during the turn. | Formats local tool/API/websocket metrics into a separator label. |
| separator presence / empty label | Not a direct notification field: local transcript flags such as `had_work_activity` and `needs_final_message_separator`, plus streaming state. | Empty label renders a plain divider; non-empty labels are joined with ` • ` and truncated to width. |
