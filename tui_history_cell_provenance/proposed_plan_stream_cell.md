# ProposedPlanStreamCell

Code paths: `PlanDelta` handling, `PlanStreamController`, commit ticks.

| Render field | Source | Transform |
|---|---|---|
| `lines` | `ServerNotification::PlanDelta.delta`. | The plan stream controller renders styled plan block fragments from the delta stream before emitting the cell. |
| `is_stream_continuation` | Not `ServerNotification`-sourced: `PlanStreamController.header_emitted`. | Marks whether this cell continues a previous streamed plan fragment. |
