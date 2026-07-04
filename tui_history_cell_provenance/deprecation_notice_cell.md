# DeprecationNoticeCell

Code paths: `ServerNotification::DeprecationNotice`.

| Render field | Source | Transform |
|---|---|---|
| `summary` | `ServerNotification::DeprecationNotice.summary`. | Copied into the cell and rendered after the red warning prefix. |
| `details` | `ServerNotification::DeprecationNotice.details`. | Optional details are rendered as dimmed wrapped continuation text. |
