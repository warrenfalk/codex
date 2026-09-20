# Choice when editing a previous prompt

Selecting a previous prompt offers two actions before changing conversation history:

- **Edit this conversation** is selected by default. Keep the same conversation ID,
  retain history strictly before the selected prompt, and discard that prompt and
  everything after it from the effective conversation.
- **Edit in a new conversation** preserves the source conversation and reopens the
  selected prompt on a new branch containing the earlier history.

Escape cancels the choice without changing history or the existing draft. Choosing
an action does not ask for another confirmation. After it succeeds, the selected
prompt opens in the composer with its text, attachments, and mention bindings.
Submitting the edited prompt continues the chosen conversation.

Replacing the first prompt empties the existing conversation's effective history
and keeps its ID. Branching from the first prompt starts a new conversation.
Replacement persists across restart and resume. Historical storage records may
remain on disk; this feature removes the abandoned suffix from effective history,
not from every stored record. Neither action undoes file edits or command effects.

The choices work with both legacy and paginated histories, including prompts
loaded from older history pages. Existing restrictions on independently editing
steering messages, in-progress prompts, and side conversations still apply.
Failures preserve the selected prompt for editing and report the error.
Successful replacement must clear stale transcript events and pagination state so
discarded turns cannot reappear.

Validation should cover the default choice, branching, cancellation, first and
later prompts, attachments, failed requests, legacy and paginated persistence,
resume after replacement, and stale buffered history. Snapshot coverage should
show the menu at normal and narrow widths and the replacement notice.
