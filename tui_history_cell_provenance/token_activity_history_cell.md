# TokenActivityHistoryCell

Code paths: local `/usage`, account token usage background request.

| Render field | Source | Transform |
|---|---|---|
| `view` | Not `ServerNotification`-sourced: local `/usage` arguments or usage-menu default. | Selects daily/weekly/cumulative chart labels and layout. |
| `state = Loading` | Not `ServerNotification`-sourced: local request lifecycle. | Renders static loading text. |
| `state = Error` | Not `ServerNotification`-sourced: local request failure / request-id matching. | Renders static unavailable text. |
| `state.Loaded.response` | Not `ServerNotification`-sourced: app-server client response to account token usage read request. | Rendered into summary and chart rows. |
| `state.Loaded.today` | Not `ServerNotification`-sourced: local `Utc::now().date_naive()` at completion time. | Anchors chart windows and suppresses future cells. |
