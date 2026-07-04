# StreamingPlanTailCell

Code paths: `PlanDelta` handling, `PlanStreamController::current_tail_display_lines`, `sync_active_stream_tail`.

| Render field | Source | Transform |
|---|---|---|
| `lines` | Accumulated `ServerNotification::PlanDelta.delta`. | Current uncommitted plan tail is rendered with plan styling and copied into the active cell. |
| `is_stream_continuation` | Not `ServerNotification`-sourced: `!PlanStreamController::tail_starts_stream()`. | Marks active-tail continuation state for transcript overlay/reflow behavior. |
