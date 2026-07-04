# StatusHistoryCell

Code paths: local `/status`, status controls, account/rate-limit update handling.

| Render field | Source | Transform |
|---|---|---|
| `model_name` | Not `ServerNotification`-sourced: local current model/session/config state. | Rendered as the primary model value. |
| `model_details` | Not `ServerNotification`-sourced: local reasoning/model detail helpers and model catalog/defaults. | Joined in parentheses after `model_name`. |
| `directory` | Usually local session/config state from start/resume/fork response; inferred child thread can use `ServerNotification::ThreadStarted.thread.cwd`. | Formatted/truncated as current directory. |
| `permissions` | Not directly `ServerNotification`-sourced: local effective approval/sandbox/permission profile state. | Rendered as permission summary. |
| `agents_summary` | Not `ServerNotification`-sourced: local instruction-source/session config state. | Read from shared lock during render and displayed in the `Agents.md` row. |
| `collaboration_mode` | Not `ServerNotification`-sourced: local config/session mode. | Optional row. |
| `model_provider` | Not `ServerNotification`-sourced in normal status output: local config/runtime base URL; inferred child thread can have provider from `ServerNotification::ThreadStarted.thread.model_provider`. | Sanitized and rendered as optional provider row. |
| `remote_connection` | Not `ServerNotification`-sourced: local app-server target/version connection state. | Optional remote row with address and version. |
| `show_chatgpt_usage_link` | Not `ServerNotification`-sourced: local auth/provider decision. | Adds ChatGPT usage link note when true. |
| `account` | `ServerNotification::AccountUpdated.{auth_mode, plan_type}`; initial bootstrap can come from non-notification account response. | Mapped to ChatGPT/API-key display text and optional row. |
| `thread_name` | `ServerNotification::ThreadNameUpdated.thread_name`; inferred initial name from `ServerNotification::ThreadStarted.thread.name`; otherwise local session state. | Optional `Thread name` row when non-empty. |
| `session_id` | Not usually `ServerNotification`-sourced: local current thread/session state; inferred child thread can use `ServerNotification::ThreadStarted.thread.session_id`. | Optional `Session` row. |
| `forked_from` | Not usually `ServerNotification`-sourced: local thread state; inferred child/forked thread can use `ServerNotification::ThreadStarted.thread.forked_from_id`. | Optional `Forked from` row when session id exists too. |
| `token_usage` | `ServerNotification::ThreadTokenUsageUpdated.token_usage.{total, last, model_context_window}`. | Converted to status token data and rendered as token/context-window rows. |
| `rate_limit_state` | `ServerNotification::AccountRateLimitsUpdated.rate_limits`; `/status` can also refresh from non-notification account/rate-limits read response. | Shared state drives current/stale/unavailable limit rows and reset/progress labels. |
