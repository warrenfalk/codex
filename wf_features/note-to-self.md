# Note To Self

## Intent

Let a user record durable personal notes in a thread without giving those notes to the agent or to future automatic context.

## Behavior

`/nts <note>` records a visible `Note to self` entry in the current thread. Leading and trailing whitespace is trimmed; internal whitespace and newlines are preserved. Empty notes are rejected.

Notes are visible in the live transcript, thread reads, transcript-style exports, and feedback uploads. A note created while a turn is running appears in that active turn without starting, steering, or queuing a model turn. A note created while the thread is idle appears in its own completed display-only turn.

Before a session starts, `/nts <note>` reports that note creation is unavailable. Bare `/nts` reports the command usage.

Notes are append-only plain text in this version.

## Model Context Boundaries

Notes are never sent as model input and are not converted into response items, turn input, user messages, context fragments, compaction input, memory extraction input, title-generation input, summary seeds, or automatic recall material.

Notes do not count as user turns for rollback. Rollback removes notes only when they fall inside the removed history region.

## Validation Expectations

Validation should cover note creation through the app-server API, persistent rollout storage as note events, thread-read reconstruction, empty-note rejection, live active-turn item notifications, idle display-only turn notifications, title and first-user-message metadata ignoring notes, rollback behavior around note-only turns, and resumed or forked model history excluding notes.
