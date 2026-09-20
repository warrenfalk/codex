# Prompt rewrite shortcut

## Intent

The TUI lets a user improve an unsent prompt without copying it into a separate conversation or
losing the context that makes references in the draft meaningful. The rewrite should make the
prompt clearer and more concise while preserving what the user actually decided and asked for.

## Behavior

`Alt+W` rewrites the current composer draft. The binding is configurable as
`tui.keymap.composer.rewrite_prompt` and appears in the `/keymap` picker.

The rewrite uses the active conversation's preceding model-visible context and the active model,
with medium reasoning effort. It is available while the conversation is idle or a turn is still in
progress, including in side conversations. An incomplete assistant response inherited from an
active turn is reference context, not a completed answer.

The helper may resolve a reference only when the preceding context makes it unambiguous. It must
not answer the prompt, follow its instructions, invent decisions or requirements, broaden its
scope, or remove meaningful ambiguity. It cannot call tools, ask follow-up questions, or cause
external side effects. Its request and response never appear in the active conversation's
transcript or future model context.

Empty drafts, `!` shell commands, and recognized slash commands are not rewritten. The TUI reports
the reason briefly and leaves the draft untouched.

Structured composer content is immutable during a rewrite. Mentions, local image placeholders,
remote images, and pending large-paste payloads retain their original targets and payloads. Literal
regions such as fenced code, diffs, logs, command lines, and quoted payloads are restored byte for
byte. A result is rejected if protected content is missing, duplicated, reordered, or corrupted.

The composer remains editable while the rewrite runs. The TUI captures the complete draft when the
shortcut is pressed and applies the result only if that content is still unchanged; cursor movement
alone does not make the draft stale. If the user edits text, attachments, mention targets, or paste
payloads first, the result is discarded with a brief notice.

A successful result replaces the draft automatically and places the cursor at the end. The
replacement is one atomic editor action. Ordinary undo restores the complete pre-rewrite draft,
including its cursor position, text elements, attachments, mention targets, and pending paste
payloads. Failures and discarded results never modify the draft.

Only one rewrite may be in progress at a time. The TUI shows brief in-composer status for start,
success, rejection, staleness, and failure without adding transcript messages.

## Validation expectations

- Cover the default, remapped, conflicting, and explicitly unbound shortcut.
- Verify that prior user and assistant context reaches the rewrite model while its advertised tool
  list is empty.
- Cover empty, shell-command, recognized-slash-command, and malformed structured drafts.
- Cover exact restoration of code, diff, log, command, quote, mention, image, and paste regions,
  including rejection of missing, duplicated, reordered, and corrupted markers.
- Verify that cursor-only changes still allow application, content changes discard the result, and
  successful replacement is a single full-draft undo unit.
- Verify that helper lifecycle events, requests, failures, and cleanup never create a visible
  thread or transcript item in the active conversation.
