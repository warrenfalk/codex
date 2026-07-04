# CompositeHistoryCell

Code paths: local command output composition and session-info composition.

| Render field | Source | Transform |
|---|---|---|
| `parts` | Not directly `ServerNotification`-sourced: local composition for `/status`, `/usage`, `/ps`, and session-info output. | Child cells are rendered in order and concatenated as one transcript entry. Any child provenance belongs to the child cell type. |
| `visibility_kind` | Not `ServerNotification`-sourced: constructor choice at the composition site. | `/status` and `/ps` composites are marked as noise; `/usage` and the session-info inner composite use normal visibility. |
