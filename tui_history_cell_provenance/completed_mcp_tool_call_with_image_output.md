# CompletedMcpToolCallWithImageOutput

Code paths: `McpToolCallCell::complete`, `try_new_completed_mcp_tool_call_with_image_output`.

| Render field | Source | Transform |
|---|---|---|
| cell presence / `_image` | `ServerNotification::ItemCompleted.item.McpToolCall.result.content[]`. | First decodable RMCP image block is base64-decoded and image-decoded; success creates this extra history cell. |
| display text | Not `ServerNotification`-sourced: fixed literal. | Always renders `tool result (image output)`; decoded image is only the presence affordance. |
