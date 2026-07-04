# NoteToSelfHistoryCell

Code paths: `ChatWidget::handle_turn_completed_notification`, `handle_thread_item`, `new_note_to_self`.

| Render field | Source | Transform |
|---|---|---|
| `note` | `ServerNotification::ItemCompleted.item.NoteToSelf.note`; special live path: `ServerNotification::TurnCompleted.turn.items[].NoteToSelf.note` when the completed turn contains only note-to-self items. Resume/read responses can provide the same `ThreadItem` outside a notification. | Copied into the cell, trailing newlines trimmed, wrapped under the fixed `Note to self` heading. |
