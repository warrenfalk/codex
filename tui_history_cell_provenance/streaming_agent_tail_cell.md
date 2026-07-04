# StreamingAgentTailCell

Code paths: `AgentMessageDelta` handling, `StreamController::current_tail_lines`, `sync_active_stream_tail`.

| Render field | Source | Transform |
|---|---|---|
| `lines` | Accumulated `ServerNotification::AgentMessageDelta.delta`. | The current uncommitted stream tail is already rendered at the controller's current width and copied into the active cell. |
| `is_first_line` | Not `ServerNotification`-sourced: `StreamController::tail_starts_stream()`. | Chooses bullet prefix versus continuation indentation. |
