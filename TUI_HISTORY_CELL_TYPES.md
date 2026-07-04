# TUI History Cell Types

Mechanical list of concrete `HistoryCell` implementations in the current checkout, ignoring factory function distinctions.

Detailed per-field provenance lives in [tui_history_cell_provenance/](tui_history_cell_provenance/README.md).

| Type | What it is |
|---|---|
| `AgentMarkdownCell` | Finalized assistant message stored as raw markdown and re-rendered on resize. |
| `AgentMessageCell` | Rendered assistant message fragment, usually from streaming output before consolidation. |
| `AgentStatusHistoryCell` | `/agent` output showing bounded previews of running sub-agent activity. |
| `CompletedMcpToolCallWithImageOutput` | Marker cell for an MCP tool result that produced image output. |
| `CompositeHistoryCell` | Container cell that renders multiple child history cells as one transcript entry. |
| `CyberPolicyNoticeCell` | Cybersecurity policy/risk notice with Trusted Access link. |
| `DeprecationNoticeCell` | Deprecation warning with summary and optional details. |
| `ExecCell` | Shell/exec command cell, including command, status, and output rendering. |
| `FinalMessageSeparator` | Turn divider, optionally showing elapsed work time and runtime metrics. |
| `HookCell` | Hook execution cell, including running/completed hook output. |
| `McpInventoryLoadingCell` | Loading/spinner cell while MCP inventory is being fetched. |
| `McpToolCallCell` | MCP tool invocation cell, including arguments, result, duration, and failures. |
| `NoteToSelfHistoryCell` | User-authored note-to-self transcript cell. |
| `PatchHistoryCell` | File-level patch/diff summary cell. |
| `PlainHistoryCell` | Raw prebuilt `Line` list, with no special wrapping beyond default behavior. |
| `PlanUpdateCell` | `update_plan` checklist/status update cell. |
| `PrefixedWrappedHistoryCell` | Text cell with initial/subsequent prefixes and width-aware wrapping. |
| `ProposedPlanCell` | Finalized proposed plan stored as markdown and re-rendered on resize. |
| `ProposedPlanStreamCell` | Already-rendered proposed-plan streaming fragment. |
| `ReasoningSummaryCell` | Reasoning summary markdown, optionally transcript-only. |
| `RequestUserInputResultCell` | Completed/interrupted `request_user_input` question and answer summary. |
| `SessionHeaderHistoryCell` | Bordered session header showing model, cwd, version, permissions, etc. |
| `SessionInfoCell` | Composite session-start card wrapping header, help text, tooltips, and model-change notices. |
| `StatusHistoryCell` | `/status` card with session/config/account/permission details. |
| `StreamingAgentTailCell` | Transient active-cell tail for mutable assistant streaming output. |
| `StreamingPlanTailCell` | Transient active-cell tail for mutable proposed-plan streaming output. |
| `TokenActivityHistoryCell` | Token activity card, backed by shared mutable loading/error/loaded state. |
| `TooltipHistoryCell` | Tip/help markdown cell used inside session info. |
| `UnifiedExecInteractionCell` | Background terminal interaction/wait summary cell. |
| `UnifiedExecProcessesCell` | Background terminal/process summary cell. |
| `UpdateAvailableHistoryCell` | Update-available notice card. |
| `UserHistoryCell` | User prompt cell, including text elements and image references. |
| `WebHyperlinkHistoryCell` | Plain lines with web URL hyperlink annotation. |
| `WebSearchCell` | Web-search activity cell, active or completed. |

## Render-Affecting Fields

Fields not listed here are lifecycle, routing, mutation, or retained data that does not directly
change rendered transcript output. `visibility_kind` affects whether a cell appears in filtered
history views, not the line content itself.

| Type | Render-affecting fields |
|---|---|
| `AgentMarkdownCell` | `markdown_source`, `cwd`, `file_opener` |
| `AgentMessageCell` | `lines`, `is_first_line` |
| `AgentStatusHistoryCell` | `entries`; nested `entries[].agent_path`, `entries[].activity` |
| `CompletedMcpToolCallWithImageOutput` | none; renders a fixed image-output marker |
| `CompositeHistoryCell` | `parts`, `visibility_kind` |
| `CyberPolicyNoticeCell` | none; renders fixed policy notice text and URL |
| `DeprecationNoticeCell` | `summary`, `details` |
| `ExecCell` | `calls`, `animations_enabled`; nested `calls[].command`, `calls[].parsed`, `calls[].output.exit_code`, `calls[].output.aggregated_output`, `calls[].output.formatted_output`, `calls[].source`, `calls[].start_time`, `calls[].duration`, `calls[].interaction_input` |
| `FinalMessageSeparator` | `elapsed_seconds`, `runtime_metrics` |
| `HookCell` | `runs`, `animations_enabled`; nested `runs[].event_name`, `runs[].status_message`, `runs[].state`, running-state `start_time`, completed-state `status`, completed-state `entries[].kind`, completed-state `entries[].text` |
| `McpInventoryLoadingCell` | `start_time`, `animations_enabled` |
| `McpToolCallCell` | `invocation`, `start_time`, `result`, `animations_enabled`; nested `invocation.server`, `invocation.tool`, `invocation.arguments`, successful `result.is_error`, successful `result.content`, error `result` string |
| `NoteToSelfHistoryCell` | `note` |
| `PatchHistoryCell` | `changes`, `cwd` |
| `PlainHistoryCell` | `lines`, `visibility_kind` |
| `PlanUpdateCell` | `explanation`, `plan`; nested `plan[].step`, `plan[].status` |
| `PrefixedWrappedHistoryCell` | `text`, `initial_prefix`, `subsequent_prefix`, `visibility_kind` |
| `ProposedPlanCell` | `plan_markdown`, `cwd` |
| `ProposedPlanStreamCell` | `lines`, `is_stream_continuation` |
| `ReasoningSummaryCell` | `content`, `cwd`, `transcript_only` |
| `RequestUserInputResultCell` | `questions`, `answers`, `interrupted`; nested `questions[].id`, `questions[].question`, `questions[].is_secret`, `questions[].options`, `answers[question_id].answers` |
| `SessionHeaderHistoryCell` | `version`, `model`, `model_style`, `reasoning_effort`, `show_fast_status`, `directory`, `yolo_mode` |
| `SessionInfoCell` | tuple field `0` containing the inner `CompositeHistoryCell` |
| `StatusHistoryCell` | `model_name`, `model_details`, `directory`, `permissions`, `agents_summary`, `collaboration_mode`, `model_provider`, `remote_connection`, `show_chatgpt_usage_link`, `account`, `thread_name`, `session_id`, `forked_from`, `token_usage`, `rate_limit_state` |
| `StreamingAgentTailCell` | `lines`, `is_first_line` |
| `StreamingPlanTailCell` | `lines`, `is_stream_continuation` |
| `TokenActivityHistoryCell` | `view`, `state`; loaded-state `response`, loaded-state `today` |
| `TooltipHistoryCell` | `tip`, `cwd` |
| `UnifiedExecInteractionCell` | `command_display`, `stdin` |
| `UnifiedExecProcessesCell` | `processes`; nested `processes[].command_display`, `processes[].recent_chunks` |
| `UpdateAvailableHistoryCell` | `latest_version`, `update_action` |
| `UserHistoryCell` | `message`, `text_elements`, `remote_image_urls` |
| `WebHyperlinkHistoryCell` | `lines`, `visibility_kind` |
| `WebSearchCell` | `query`, `action`, `start_time`, `completed`, `animations_enabled` |
