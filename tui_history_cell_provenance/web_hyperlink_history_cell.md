# WebHyperlinkHistoryCell

Code paths: local feedback submission success rendering.

| Render field | Source | Transform |
|---|---|---|
| `lines` | Not `ServerNotification`-sourced: local `FeedbackSubmitted` result, thread id, category, audience, and returned feedback links. | Builds a confirmation message and optional GitHub/internal URLs; URL spans are annotated as web links. |
| `visibility_kind` | Not `ServerNotification`-sourced: constructor choice for feedback output. | Feedback success output is marked as noise. |
