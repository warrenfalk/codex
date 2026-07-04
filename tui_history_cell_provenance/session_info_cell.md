# SessionInfoCell

Code paths: `new_session_info`.

| Render field | Source | Transform |
|---|---|---|
| tuple field `0` / inner composite | Not a direct notification payload. Built from local config, requested model, `ThreadSessionState`, first-event state, tooltip state, auth plan, and fast-status state. Some inferred child-thread session values can originate from `ServerNotification::ThreadStarted.thread.{cwd, name, model_provider}`. | Delegates all rendering to the inner `CompositeHistoryCell`. |
| header part | Same sources as `SessionHeaderHistoryCell`. | First component of the composite. |
| startup help part | Not `ServerNotification`-sourced: local `is_first_event`. | Adds static startup help rows. |
| tooltip part | Not `ServerNotification`-sourced: `config.show_tooltips`, optional override, and local `tooltips::get_tooltip(auth_plan, show_fast_status)`. | Adds a `TooltipHistoryCell`. |
| model-changed notice part | Not `ServerNotification`-sourced: local comparison `requested_model != session.model`. | Adds static `model changed` lines. |
