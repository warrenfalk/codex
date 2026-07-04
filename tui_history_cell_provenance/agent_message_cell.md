# AgentMessageCell

Code paths: `AgentMessageDelta` handling, `StreamController`, commit ticks.

| Render field | Source | Transform |
|---|---|---|
| `lines` | `ServerNotification::AgentMessageDelta.delta`; fallback path can seed streaming from `ServerNotification::ItemCompleted.item.AgentMessage.text` when no stream controller exists. Resume/read `ThreadItem::AgentMessage.text` can enter the same completion path outside a notification. | The stream controller parses/renders markdown into stable `HyperlinkLine`s before the cell is emitted. |
| `is_first_line` | Not `ServerNotification`-sourced: `StreamController.header_emitted` state. | First emitted chunk receives the bullet prefix; later emitted chunks render as continuation lines. |
