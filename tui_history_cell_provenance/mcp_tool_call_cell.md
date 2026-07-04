# McpToolCallCell

Code paths: `ServerNotification::ItemStarted.item.McpToolCall`, `ServerNotification::ItemCompleted.item.McpToolCall`, `new_active_mcp_tool_call`.

| Render field | Source | Transform |
|---|---|---|
| `invocation.server` | `ServerNotification::ItemStarted.item.McpToolCall.server`; fallback/orphan completion from `ServerNotification::ItemCompleted.item.McpToolCall.server`. | Rendered in cyan as the `<server>` part of `<server>.<tool>(args)`. |
| `invocation.tool` | `ServerNotification::ItemStarted.item.McpToolCall.tool`; fallback/orphan completion from `ItemCompleted.item.McpToolCall.tool`. | Rendered in cyan as the tool name. |
| `invocation.arguments` | `ServerNotification::ItemStarted.item.McpToolCall.arguments`; fallback/orphan completion from `ItemCompleted.item.McpToolCall.arguments`. | Wrapped in `Some`, compact-JSON stringified, dimmed inside parentheses. |
| `result` success/error | `ServerNotification::ItemCompleted.item.McpToolCall.result.{content, structured_content}` or `.error.message`. | Error becomes `Err(message)`; result becomes `CallToolResult { is_error: Some(false), ... }`; status controls header (`Calling`/`Called`) and bullet color. |
| result content lines | `ServerNotification::ItemCompleted.item.McpToolCall.result.content[]`. | Parsed as RMCP content: text is truncated/wrapped, image/audio become placeholders, resources/links render URI summaries, invalid blocks fall back to JSON. |
| `start_time` | Not `ServerNotification`-sourced: `Instant::now()` when active cell is constructed. | Drives spinner timing while `result` is `None`. |
| `animations_enabled` | Not `ServerNotification`-sourced: `self.config.animations`. | Selects animated versus static active indicator. |
| interrupted failure state | Indirectly caused by `ServerNotification::TurnCompleted.turn.status == Interrupted`, but the cell value is local finalization state. | Active MCP cell is marked failed with `Err("interrupted")`. |

Non-render fields: `call_id` routes completion to the active cell; `duration` is set from `McpToolCall.duration_ms` but is not read by the current render methods.
