# PatchHistoryCell

Code paths: `ServerNotification::ItemStarted.item.FileChange`, `on_patch_apply_begin`, `new_patch_event`.

| Render field | Source | Transform |
|---|---|---|
| `changes` | `ServerNotification::ItemStarted.item.FileChange.changes[].{path, kind, diff}`. | Converted with `file_update_changes_to_display` into local `HashMap<PathBuf, FileChange>`, then rendered by `create_diff_summary`. |
| `cwd` | Not `ServerNotification`-sourced: `self.config.cwd` at construction. | Used to display changed paths relative to the session cwd. |

Related but not this cell: `ServerNotification::ItemCompleted.item.FileChange.status == Failed` creates a separate `PlainHistoryCell` failure notice.
