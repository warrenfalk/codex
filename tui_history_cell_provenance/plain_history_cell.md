# PlainHistoryCell

Code paths: generic `new_event`, `new_error_event`, image notices, review-mode notices, sub-agent/collab-agent notices, fallback transcript rendering.

| Render field | Source | Transform |
|---|---|---|
| `lines` | `ServerNotification::Error.error.message` when the error is not retrying and is not handled by the cyber-policy or server-overloaded specialized cells. | Rendered as a red error line. Local rate-limit state can replace some rate-limit messages with fixed quota text before construction. |
| `lines` | `ServerNotification::ItemCompleted.item.FileChange.status` when the status is failed. | The failed file-change item emits a fixed `Failed to apply patch` line. |
| `lines` | `ServerNotification::ItemCompleted.item.ImageView.path`. | The path is relativized against the local working directory and rendered as an image-view event line. |
| `lines` | `ServerNotification::ItemCompleted.item.ImageGeneration.{id, status, revised_prompt, saved_path}`. | The generation status controls the event line; `revised_prompt` is shown when present, otherwise `id` is used; `saved_path` adds an optional saved-file line. |
| `lines` | `ServerNotification::{ItemStarted, ItemCompleted}.item.CollabAgentToolCall.{tool, status, receiver_thread_ids, prompt, model, reasoning_effort, agents_states}`. | Collab-agent activity is summarized into fixed-prefix lines; prompts and nested agent state details are truncated/enriched with local agent metadata. |
| `lines` | `ServerNotification::{ItemStarted, ItemCompleted}.item.SubAgentActivity.{kind, agent_path}`. | Sub-agent activity is summarized into transcript event lines; local agent metadata can refine the visible label. |
| `lines` | `ServerNotification::{ItemStarted, ItemCompleted}.item.{EnteredReviewMode, ExitedReviewMode}`. | Emits fixed review-mode status lines. |
| `lines` | `ServerNotification::ItemCompleted.item.ContextCompaction`. | Emits a fixed `Context compacted` line. |
| `lines` | Not `ServerNotification`-sourced: fallback transcript rendering for response items and local app/config/input/feedback/session events. | Prebuilt `Line` values are stored directly and rendered without cell-specific transformation. |
| `visibility_kind` | Not `ServerNotification`-sourced: constructor choice at the event site. | Controls whether the cell participates in filtered/noise transcript views. |
