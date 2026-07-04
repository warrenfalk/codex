# PrefixedWrappedHistoryCell

Code paths: warning/config/model-verification notices, server-overloaded errors, guardian approval review completion, helper rendering inside `WebSearchCell`.

| Render field | Source | Transform |
|---|---|---|
| `text` | `ServerNotification::Warning.message`; `ServerNotification::GuardianWarning.message`. | Warning text is wrapped with warning prefixes and rendered in yellow. |
| `text` | `ServerNotification::ConfigWarning.{summary, details}`. | `summary` and `details` are joined as warning text. |
| `text` | `ServerNotification::ModelVerification.verifications[]` when a verification is `TrustedAccessForCyber`. | The boundary value selects a fixed Trusted Access warning message. |
| `text` | `ServerNotification::Error.error.{message, codex_error_info}` when `codex_error_info` is `ServerOverloaded`. | Non-empty messages are rendered directly; empty messages are replaced with fixed high-load text. |
| `text` | `ServerNotification::ItemGuardianApprovalReviewCompleted.{review, action}`. | Terminal approved/denied/timed-out review outcomes are converted into a short approval result summary. |
| `text` | Helper-only path from `ServerNotification::{ItemStarted, ItemCompleted}.item.WebSearch.{query, action}` through `WebSearchCell::display_lines`. | Web-search rendering temporarily constructs this cell to wrap query/action detail text; the `PrefixedWrappedHistoryCell` itself is not stored in history for that path. |
| `initial_prefix` | Not `ServerNotification`-sourced: constructor choice for the event type. | Warnings use a warning prefix; approval results use success/failure glyphs; web-search helper rendering uses active/static search prefixes. |
| `subsequent_prefix` | Not `ServerNotification`-sourced: constructor choice for wrapped continuation lines. | Continuation lines align under the initial prefix. |
| `visibility_kind` | Not `ServerNotification`-sourced: constructor choice for the event type. | Most warning/approval/search helper uses are marked as noise. |
