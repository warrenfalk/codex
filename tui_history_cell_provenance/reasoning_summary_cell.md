# ReasoningSummaryCell

Code paths: `handle_server_notification`, `on_agent_reasoning_delta`, `on_reasoning_section_break`, `on_agent_reasoning_final`, `new_reasoning_summary_block`.

| Render field | Source | Transform |
|---|---|---|
| `content` | Live deltas: `ServerNotification::ReasoningSummaryTextDelta.delta`; additionally `ServerNotification::ReasoningTextDelta.delta` when raw reasoning display is enabled. Replay carriers: `ServerNotification::ItemCompleted.item.Reasoning.summary[]` and optionally `.content[]`, or equivalent `TurnCompleted.turn.items[]` / resume-read `ThreadItem`s. | Deltas are appended into reasoning buffers; `ReasoningSummaryPartAdded` inserts a blank section break; finalization builds one markdown summary cell. |
| `cwd` | Not `ServerNotification`-sourced directly: local `self.config.cwd` / active transcript cwd at finalization, or thread cwd from non-notification resume/read session state. | Snapshotted for local file-link rendering in the reasoning body. |
| `transcript_only` | Not `ServerNotification`-sourced: local heuristic in `new_reasoning_summary_block`. | If the reasoning text only contributes transcript content after header extraction, the cell can be hidden from normal display while remaining available for transcript/raw rendering. |
