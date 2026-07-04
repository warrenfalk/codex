# CyberPolicyNoticeCell

Code paths: non-retry error handling for cyber-policy errors.

| Render field | Source | Transform |
|---|---|---|
| cell presence / fixed notice content | `ServerNotification::Error.error.codex_error_info` when it is `CyberPolicy`. | The boundary value selects this specialized cell; the rendered text and `https://chatgpt.com/cyber` URL are fixed locally and do not use `Error.error.message`. |
