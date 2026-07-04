# UserHistoryCell

Code paths: `ChatWidget::handle_thread_item`, `on_committed_user_message`, `new_user_prompt`.

| Render field | Source | Transform |
|---|---|---|
| `message` | `ServerNotification::ItemCompleted.item.UserMessage.content[].Text.text`; also `ServerNotification::TurnCompleted.turn.items[].UserMessage.content[].Text.text` when complete turns are replayed. Resume/read responses can provide the same `ThreadItem` outside a notification. | Text inputs are concatenated into the displayed prompt; prompt-context prefixes can be stripped before construction; display wraps with the user-message prefix. |
| `text_elements` | `ServerNotification::ItemCompleted.item.UserMessage.content[].Text.text_elements`; also `TurnCompleted.turn.items[]` replay carriers and non-notification resume/read `ThreadItem`s. | Byte ranges are rebased after concatenation/stripping and rendered with element styling. |
| `remote_image_urls` | `ServerNotification::ItemCompleted.item.UserMessage.content[].Image.url`; also equivalent replay/resume `ThreadItem` carriers. | URLs themselves are not displayed; the count produces numbered image labels. |

Non-render field: `local_image_paths` is retained on the cell but is not read by `display_lines` or `raw_lines`.
