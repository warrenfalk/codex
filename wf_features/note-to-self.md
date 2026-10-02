# Note To Self

## Intent

Let a user record durable personal notes in a thread without giving those notes to the agent or to future automatic context.

## Behavior

`/nts <note>` records a visible `Note to self` entry in the current thread. Leading and trailing whitespace is trimmed; internal whitespace and newlines are preserved. Empty notes are rejected.

Notes are visible in the live transcript, thread reads, transcript-style exports, and feedback uploads. A note created while a turn is running appears in that active turn without starting, steering, or queuing a model turn. A note created while the thread is idle appears in its own completed display-only turn.

Before a session starts, `/nts <note>` reports that note creation is unavailable.

Bare `/nts` opens a notes-only, full-screen reader. It shows every note in the current thread, newest first, including notes from older saved history and personal side-conversation summaries. Each note is shown in full with wrapped text and preserved blank lines. The standard transcript pager keys scroll, page, and jump to the beginning or end; Escape closes the reader and returns to the conversation. Browsing works during a running turn and preserves the composer and running work.

An empty reader says `No notes yet. Add one with /nts <note>.` History loads without blocking interaction; the reader indicates loading or failure rather than claiming an incomplete list is complete. Closing and reopening `/nts` retries a failed load. New notes appear in the open reader.

When the thread has notes, a persistent pink/magenta line immediately above the composer shows `1 note to self · /nts to view` or `<count> notes to self · /nts to view`. The count covers the entire saved thread, independently of the visible transcript. While history is still loading, known notes show a reminder without an incomplete numeric count. A failed load shows a retry hint. The line disappears when there are no notes; viewing notes does not clear it. Switching, resuming, forking, or reverting a thread refreshes its notes without retaining notes removed from that thread or importing notes from a different thread.

Notes are append-only plain text in this version.

## Model Context Boundaries

Notes are never sent as model input and are not converted into response items, turn input, user messages, context fragments, compaction input, memory extraction input, title-generation input, summary seeds, or automatic recall material.

Notes do not count as user turns for rollback. Rollback removes notes only when they fall inside the removed history region.

## Validation Expectations

Validation should cover note creation through the app-server API, persistent rollout storage as note events, thread-read reconstruction, empty-note rejection, live active-turn item notifications, idle display-only turn notifications, title and first-user-message metadata ignoring notes, rollback behavior around note-only turns, and resumed or forked model history excluding notes. Reader coverage includes full history beyond the first page, newest-first ordering, multiline and narrow-terminal rendering, empty/loading/error states, navigation and closing, live-note deduplication during loading, thread changes and stale load results, and the persistent indicator during idle and running turns.
