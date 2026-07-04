# TUI History Cell Provenance

This directory traces render-affecting `HistoryCell` fields back to the app-server boundary.

Boundary rule: stop at `ServerNotification`. If a value is read from a `ThreadItem` carried inside
a notification, cite the notification and the nested `ThreadItem` field. Do not trace into how the
app server produced the notification field.

Use this shape for each cell file:

```md
# CellName

| Render field | Source | Transform |
|---|---|---|
| `field` | `ServerNotification::Variant.field.path` | Brief copy/derive/render note. |
```

Use `not ServerNotification-sourced` for values derived from local TUI config, local command state,
static text, wall-clock/animation timing, shared handles, or other local state.

Source notation uses the carrier notification plus the nested item path when the app server sends
transcript items in bulk:

- `ServerNotification::ItemStarted.item.<ThreadItem>`
- `ServerNotification::ItemCompleted.item.<ThreadItem>`
- `ServerNotification::TurnStarted.turn.items[].<ThreadItem>`
- `ServerNotification::TurnCompleted.turn.items[].<ThreadItem>`

Some resume/read/start response paths provide `ThreadItem`s outside `ServerNotification`; those
paths are marked as not `ServerNotification`-sourced or as equivalent non-notification replay
carriers.

## Cell Files

| Cell | File |
|---|---|
| `AgentMarkdownCell` | [agent_markdown_cell.md](agent_markdown_cell.md) |
| `AgentMessageCell` | [agent_message_cell.md](agent_message_cell.md) |
| `AgentStatusHistoryCell` | [agent_status_history_cell.md](agent_status_history_cell.md) |
| `CompletedMcpToolCallWithImageOutput` | [completed_mcp_tool_call_with_image_output.md](completed_mcp_tool_call_with_image_output.md) |
| `CompositeHistoryCell` | [composite_history_cell.md](composite_history_cell.md) |
| `CyberPolicyNoticeCell` | [cyber_policy_notice_cell.md](cyber_policy_notice_cell.md) |
| `DeprecationNoticeCell` | [deprecation_notice_cell.md](deprecation_notice_cell.md) |
| `ExecCell` | [exec_cell.md](exec_cell.md) |
| `FinalMessageSeparator` | [final_message_separator.md](final_message_separator.md) |
| `HookCell` | [hook_cell.md](hook_cell.md) |
| `McpInventoryLoadingCell` | [mcp_inventory_loading_cell.md](mcp_inventory_loading_cell.md) |
| `McpToolCallCell` | [mcp_tool_call_cell.md](mcp_tool_call_cell.md) |
| `NoteToSelfHistoryCell` | [note_to_self_history_cell.md](note_to_self_history_cell.md) |
| `PatchHistoryCell` | [patch_history_cell.md](patch_history_cell.md) |
| `PlainHistoryCell` | [plain_history_cell.md](plain_history_cell.md) |
| `PlanUpdateCell` | [plan_update_cell.md](plan_update_cell.md) |
| `PrefixedWrappedHistoryCell` | [prefixed_wrapped_history_cell.md](prefixed_wrapped_history_cell.md) |
| `ProposedPlanCell` | [proposed_plan_cell.md](proposed_plan_cell.md) |
| `ProposedPlanStreamCell` | [proposed_plan_stream_cell.md](proposed_plan_stream_cell.md) |
| `ReasoningSummaryCell` | [reasoning_summary_cell.md](reasoning_summary_cell.md) |
| `RequestUserInputResultCell` | [request_user_input_result_cell.md](request_user_input_result_cell.md) |
| `SessionHeaderHistoryCell` | [session_header_history_cell.md](session_header_history_cell.md) |
| `SessionInfoCell` | [session_info_cell.md](session_info_cell.md) |
| `StatusHistoryCell` | [status_history_cell.md](status_history_cell.md) |
| `StreamingAgentTailCell` | [streaming_agent_tail_cell.md](streaming_agent_tail_cell.md) |
| `StreamingPlanTailCell` | [streaming_plan_tail_cell.md](streaming_plan_tail_cell.md) |
| `TokenActivityHistoryCell` | [token_activity_history_cell.md](token_activity_history_cell.md) |
| `TooltipHistoryCell` | [tooltip_history_cell.md](tooltip_history_cell.md) |
| `UnifiedExecInteractionCell` | [unified_exec_interaction_cell.md](unified_exec_interaction_cell.md) |
| `UnifiedExecProcessesCell` | [unified_exec_processes_cell.md](unified_exec_processes_cell.md) |
| `UpdateAvailableHistoryCell` | [update_available_history_cell.md](update_available_history_cell.md) |
| `UserHistoryCell` | [user_history_cell.md](user_history_cell.md) |
| `WebHyperlinkHistoryCell` | [web_hyperlink_history_cell.md](web_hyperlink_history_cell.md) |
| `WebSearchCell` | [web_search_cell.md](web_search_cell.md) |
