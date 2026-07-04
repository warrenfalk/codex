# WebSearchCell

Code paths: `ServerNotification::ItemStarted.item.WebSearch`, `ServerNotification::ItemCompleted.item.WebSearch`, `new_active_web_search_call`, `new_web_search_call`.

| Render field | Source | Transform |
|---|---|---|
| `query` | Completed path: `ServerNotification::ItemCompleted.item.WebSearch.query`. Active start currently ignores `ItemStarted.item.WebSearch.query` and uses an empty string. | Used as display detail only when the action detail is empty. |
| `action` | `ServerNotification::ItemCompleted.item.WebSearch.action`. | `None` becomes `WebSearchAction::Other`; action variant chooses query, first query, URL, or pattern/URL text. |
| `completed` | Lifecycle derived from `ServerNotification::ItemStarted.item.WebSearch.id` and `ServerNotification::ItemCompleted.item.WebSearch.id`. | Controls header text (`Searching the web` vs `Searched the web`) and active/static bullet. |
| `start_time` | Not `ServerNotification`-sourced: `Instant::now()` when cell is constructed. | Drives active spinner timing while `completed == false`. |
| `animations_enabled` | Active path: not `ServerNotification`-sourced, `self.config.animations`; orphan completed path uses `false`. | Selects animated versus static active indicator. |

Non-render field: `call_id` is used to match completion to the active cell, but it is not displayed.
