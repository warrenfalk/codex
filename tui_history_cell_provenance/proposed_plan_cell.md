# ProposedPlanCell

Code paths: `PlanDelta` handling, `on_plan_item_completed`, `AppEvent::ConsolidateProposedPlan`.

| Render field | Source | Transform |
|---|---|---|
| `plan_markdown` | Preferred final source: `ServerNotification::ItemCompleted.item.Plan.text`; fallback/consolidation source: accumulated `ServerNotification::PlanDelta.delta`. Equivalent completed plan items can arrive through `TurnCompleted.turn.items[]` or non-notification resume/read responses. | Stored as raw markdown and re-rendered later inside the `Proposed Plan` frame. |
| `cwd` | Not `ServerNotification`-sourced directly: local `self.config.cwd` at construction/consolidation, or thread cwd from replay/session state. | Snapshotted for local file-link rendering inside the plan body. |
