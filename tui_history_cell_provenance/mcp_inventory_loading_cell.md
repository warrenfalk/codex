# McpInventoryLoadingCell

Code paths: local `/mcp`, `ChatWidget::add_mcp_output`, MCP inventory background request.

| Render field | Source | Transform |
|---|---|---|
| cell lifetime | Not `ServerNotification`-sourced: local `/mcp` command flow and `AppEvent::FetchMcpInventory`. | Inserted as active loading cell before inventory fetch; cleared when inventory result handling completes. |
| `start_time` | Not `ServerNotification`-sourced: `Instant::now()` when constructed. | Drives spinner phase and transcript animation tick. |
| `animations_enabled` | Not `ServerNotification`-sourced: `self.config.animations`. | Selects animated versus static activity indicator. |
| display text | Not `ServerNotification`-sourced: fixed literal. | Renders `Loading MCP inventory...`/ellipsis text. |
