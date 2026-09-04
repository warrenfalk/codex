# Automatic Thread Title

## What it adds

This feature teaches Codex to generate a short thread title automatically once a
thread has enough signal to name itself. The goal is to make saved sessions
easier to recognize without forcing the user to rename every thread manually.

## Final behavior

- If a thread is still unnamed, Codex looks at the first user message and the
  latest assistant reply and asks the model for a concise title.
- The title generator is tightly constrained: plain text only, JSON output,
  under eight words, and focused on the user's task rather than the assistant.
- If the generated title normalizes to the same text as the first user message,
  Codex keeps the thread unnamed instead of storing a low-value duplicate.
- Generated titles are persisted into the state DB and then show up anywhere
  thread names are read back, including session lookup flows.
- A manual rename still wins. If the user names the thread while the automatic
  title request is still in flight, the manual title stays in place.

## Why it matters

The thread list no longer has to rely only on raw first-message previews. Short,
stable titles make resume and session-management flows much easier once a user
has accumulated many saved threads.

Original implementation commit: `b495f7405e` (`core: automatic thread title`)
