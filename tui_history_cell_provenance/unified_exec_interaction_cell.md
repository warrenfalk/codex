# UnifiedExecInteractionCell

Code paths: `ServerNotification::TerminalInteraction`, unified-exec process tracking, `new_unified_exec_interaction`.

| Render field | Source | Transform |
|---|---|---|
| `stdin` | `ServerNotification::TerminalInteraction.stdin`. | Empty string renders as a background-terminal wait; non-empty stdin renders wrapped input lines. |
| `command_display` | `ServerNotification::TerminalInteraction.process_id` selects local process state seeded by `ServerNotification::ItemStarted.item.CommandExecution.{id, process_id, command, source}` for unified-exec startup commands. | The tracked command is split and shell-wrapper stripped; optional display is appended to wait/interaction headers. |
