# AgentStatusHistoryCell

Code paths: local `/agent`, `agent_status_feed`, per-thread event stores.

| Render field | Source | Transform |
|---|---|---|
| `entries` | Not directly `ServerNotification`-sourced: local `/agent` status reads agent navigation and each subagent `ThreadEventStore`. Those stores are populated by per-thread notifications. | Empty list renders no-running-subagents state; otherwise one section per running sub-agent. |
| `entries[].agent_path` | Local agent navigation cache. It can be seeded from `ServerNotification::ThreadStarted.thread.{agent_nickname, agent_role}` and from `ThreadItem::SubAgentActivity.agent_path` inside `ItemStarted`/`ItemCompleted` notifications. | Rendered as the sub-agent title. |
| `entries[].activity` | `ServerNotification::ItemStarted.item` and `ServerNotification::ItemCompleted.item` buffered in the subagent's `ThreadEventStore`. | Deduplicates by item id, keeps newest bounded entries, then restores chronological order. |
| activity summary text | Nested `ThreadItem` fields inside the buffered notifications: message/plan/reasoning text, command, file-change count, MCP/dynamic/collab tool names, sub-agent kind/path, web query, image path; some variants are static or ignored. | Whitespace-normalized and truncated for preview. |
