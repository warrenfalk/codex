# PlanUpdateCell

Code paths: `ServerNotification::TurnPlanUpdated`, `on_plan_update`, `new_plan_update`.

| Render field | Source | Transform |
|---|---|---|
| `explanation` | `ServerNotification::TurnPlanUpdated.explanation`. | Stored as optional note text, trimmed for display, then wrapped in dim italic style. |
| `plan[].step` | `ServerNotification::TurnPlanUpdated.plan[].step`. | Copied into `PlanItemArg.step` and rendered as checklist text. |
| `plan[].status` | `ServerNotification::TurnPlanUpdated.plan[].status`. | App-server status enum is mapped to `Pending`, `InProgress`, or `Completed`, then rendered with checkbox/cross-out styling. |
