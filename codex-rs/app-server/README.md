# Fork app-server API extensions

## Firehose observer

`event/firehose` marks the current initialized connection as a passive observer. Params may be omitted and the response is `{}`:

```json
{ "method": "event/firehose", "id": 17 }
{ "id": 17, "result": {} }
```

After subscribing, the connection receives one observed copy of each logical outbound app-server event. Thread-scoped notifications and requests are observed even when the thread has zero normal subscribers; ordinary connections without a thread subscription do not receive those events. Notifications are delivered with their original method and params. Server-initiated requests are delivered as `serverRequest/observed` notifications:

```json
{ "method": "serverRequest/observed", "params": {
    "request": { "method": "item/tool/requestUserInput", "id": 9, "params": { "...": "..." } }
} }
```

The observer connection cannot answer requests it only observed; JSON-RPC responses or errors for those ids are ignored. Firehose delivery ignores `optOutNotificationMethods`. The subscription does not add the connection to any thread subscriber set, does not replay pending thread requests, and does not keep single-client app-server mode alive by itself.

Subscription is idempotent and lasts until the connection closes. Explicit delivery to a connection takes precedence over observation, so a client that both participates and observes receives only one copy.

See [the feature contract](../../wf_features/app-server-firehose-subscription.md) for the behavior and validation expectations this fork preserves.

## Trust sandbox approval policies

`approvalPolicy` also accepts `"trust-sandbox"` and `"trust-sandbox-timeout"`. Both trust managed restricted sandbox enforcement for dangerous command-shape fallback heuristics while still prompting for sandbox overrides and explicit exec-policy prompt rules. `"trust-sandbox-timeout"` additionally auto-approves sandbox-override command prompts after 300 seconds without persisting session approval or policy amendments.

## Project environment loading

Local `thread/shellCommand` calls accept `projectEnv: "auto" | "bypass"`, defaulting to `"auto"`. Auto mode loads the cwd's direnv environment and fails before launch if loading fails; bypass skips loading for that command. The top-level `disable_project_env` config key disables loading entirely.

`thread/projectEnv/read` accepts `{ "threadId": "..." }` and returns the canonical local cwd's current status. `thread/projectEnv/statusChanged` reports changes separately from thread lifecycle notifications, using `disabled`, `none`, `building`, `ready`, or `failed` states. Standalone `command/exec` is unaffected. See [the feature contract](../../wf_features/project-environment-loading.md) for environment precedence, cancellation, and status payload expectations.

## Note to self

`thread/note/create` appends a visible note without starting or steering an agent turn. It trims outer whitespace, preserves internal whitespace, and rejects empty notes with JSON-RPC `-32602`.

```json
{ "method": "thread/note/create", "id": 24, "params": {
    "threadId": "thr_123", "note": "Check logs before retrying."
} }
{ "id": 24, "result": {
    "turnId": "turn_note_1",
    "item": { "type": "noteToSelf", "id": "item_note_1", "note": "Check logs before retrying." }
} }
```

During an active turn, the note is appended to that turn and emits `item/completed`. For an idle thread, it appears in its own completed display-only turn and emits `turn/completed`. Notes persist in thread reads and transcript-style history. They remain excluded from model context, compaction, memory extraction, and title metadata. The TUI exposes this behavior as `/nts <note>`; see [the feature contract](../../wf_features/note-to-self.md).

## Model-only turns

Experimental `turn/startModelOnly` starts an idle turn using the target thread's model-visible history with no tools, hooks, skill/plugin injection, memory startup, or automatic title generation. It accepts `threadId`, `input`, `model`, optional `effort`, and optional `outputSchema`, and returns the initial `turn` with the usual turn/item notifications. Parent-owned Multi-Agent V2 subagents reject direct turns.

The input and output become ordinary history on the target thread. Clients needing a hidden transformation must use a separate ephemeral thread, as the TUI's [prompt rewrite shortcut](../../wf_features/prompt-rewrite-shortcut.md) does.
