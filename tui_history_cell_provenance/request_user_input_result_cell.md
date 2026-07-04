# RequestUserInputResultCell

Code paths: `ServerRequest::ToolRequestUserInput`, request-user-input overlay, response submission.

| Render field | Source | Transform |
|---|---|---|
| `questions` | Not `ServerNotification`-sourced: `ServerRequest::ToolRequestUserInput.params.questions`. | Stored from the interactive request; question text renders rows, `is_secret` masks answers, and `options` changes whether freeform text is labeled as `answer` or `note`. |
| `answers` | Not `ServerNotification`-sourced: local request-user-input overlay state, later sent as `ToolRequestUserInputResponse.answers`. | Selected option labels plus optional `user_note: ...` entries are split into answer rows and note rows. |
| `interrupted` | Not `ServerNotification`-sourced: local constructor value. | Adds interrupted/unanswered summary text when true; current normal submit/auto-resolution paths construct completed cells with `false`. |
