# Side Conversation Close Summary

## Intent

Side conversations are ephemeral forks for lightweight exploration. Closing one should avoid accidental loss while still letting useful findings move back into the parent thread.

## Behavior

When the user presses Ctrl+C in an idle side conversation with an empty composer and no active overlay or popup, the TUI opens a close prompt instead of immediately returning to the parent. The prompt has four choices:

- Cancel: dismisses the prompt and keeps the side conversation active.
- Summarize: asks the side agent to summarize the side conversation and saves that summary into the parent thread as model-visible assistant history.
- Summarize for me: asks the side agent to summarize the side conversation and saves that summary into the parent thread as a personal `Note to self`.
- Leave: discards the side conversation without saving a summary and returns to the parent.

Cancel is selected by default. Pressing Enter accepts the highlighted choice. Pressing Esc or Ctrl+C while the prompt is open acts like Cancel.

If a side turn is running, Ctrl+C interrupts that turn first. A later Ctrl+C after the side thread is idle opens the close prompt. Existing composer, overlay, modal, popup, and normal cancellation behavior continue to take priority over this prompt.

Only one side-close action can be in progress at a time. While summarization is running, the TUI stays on the side conversation and shows the normal side-thread progress.

## Summary Persistence

Both summary actions submit the same side-only instruction that asks the side agent to summarize only messages after the side-conversation boundary. Inherited parent history before that boundary is context only and must not be summarized as side-conversation work.

On successful `Summarize` completion, the final assistant text from the summary turn is injected into the parent thread as a durable, model-visible assistant message headed:

```text
Side conversation summary
```

The parent TUI transcript shows the saved summary as an assistant message with the same heading.
Every live TUI session viewing the parent thread shows the saved summary immediately, including sessions other than the one that created and closed the side conversation.
The live summary is a display item only: saving it does not start or complete a parent-thread turn.
Saved parent threads also replay the summary in the transcript when their history is read again.

On successful `Summarize for me` completion, the final assistant text from the summary turn is saved in the parent thread through the normal note-to-self path with this note body:

```text
Side conversation summary

<generated summary>
```

The personal summary note is visible anywhere ordinary notes are visible, including the transcript, thread reads, exports, and feedback uploads. It must never become model input, response history, compaction input, title-generation input, memory extraction input, automatic recall material, or a future summary seed.

The side thread is discarded only after the parent save succeeds. For `Summarize`, that means the parent injection succeeds and the parent transcript has received the visible summary item. For `Summarize for me`, that means the parent note-to-self creation succeeds.

## Failure Handling

If summarization fails, is interrupted, produces no assistant text, parent injection fails, or parent note creation fails, the side conversation remains open and the TUI shows an error. The side thread must not be discarded on these failures.

## Validation

Validation should cover the close prompt default, Cancel/Esc/Ctrl+C behavior, both summary choices, Leave preserving the discard path, running-turn interrupt priority, unchanged composer and popup behavior, successful assistant-summary injection and discard, successful personal-summary note creation and discard, failure cases that keep the side conversation open, immediate summary display in every subscribed session without a synthetic turn completion, and silent hidden-context injection.
