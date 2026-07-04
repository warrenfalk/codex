# AgentMarkdownCell

Code paths: `on_agent_message_item_completed`, `flush_answer_stream_with_separator`, `AppEvent::ConsolidateAgentMessage`.

| Render field | Source | Transform |
|---|---|---|
| `markdown_source` | Final item: `ServerNotification::ItemCompleted.item.AgentMessage.text`; streaming source: accumulated `ServerNotification::AgentMessageDelta.delta`. Equivalent completed `ThreadItem`s can arrive through `TurnCompleted.turn.items[]` or non-notification resume/read responses. | Assistant markdown is parsed to visible markdown; trailing `AgentMessageCell`s are consolidated into one source-backed markdown cell. |
| `cwd` | Not `ServerNotification`-sourced directly: local `self.config.cwd` / active transcript cwd at consolidation, or thread cwd from session/replay state. | Snapshotted so local file links continue to render relative to the session that produced the message. |
| `file_opener` | Not `ServerNotification`-sourced: TUI config. | Passed into markdown link rendering for clickable local links. |
