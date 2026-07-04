# UpdateAvailableHistoryCell

Code paths: startup update check.

| Render field | Source | Transform |
|---|---|---|
| `latest_version` | Not `ServerNotification`-sourced: local cached update information from `updates::get_upgrade_version(&config)`. | Rendered as the version available for upgrade. |
| `update_action` | Not `ServerNotification`-sourced: local install/update context from `get_update_action()`. | Selects the visible update instruction text. |
