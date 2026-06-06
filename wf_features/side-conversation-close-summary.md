# Side Conversation Close Summary

## Intent

Side conversations are ephemeral forks for lightweight exploration. Closing one should avoid accidental loss while still letting useful findings move back into the parent thread.

## Behavior

When the user presses Ctrl+C in an idle side conversation with an empty composer and no active overlay or popup, the TUI opens a close prompt instead of immediately returning to the parent. The prompt has three choices:

- Cancel: dismisses the prompt and keeps the side conversation active.
- Summarize: asks the side agent to summarize the side conversation and saves that summary into the parent thread.
- Leave: discards the side conversation without saving a summary and returns to the parent.

Cancel is selected by default. Pressing Enter accepts the highlighted choice. Pressing Esc or Ctrl+C while the prompt is open acts like Cancel.

If a side turn is running, Ctrl+C interrupts that turn first. A later Ctrl+C after the side thread is idle opens the close prompt. Existing composer, overlay, modal, popup, and normal cancellation behavior continue to take priority over this prompt.

Only one side-close action can be in progress at a time. While summarization is running, the TUI stays on the side conversation and shows the normal side-thread progress.

## Summary Persistence

Summarize submits a side-only instruction that asks the side agent to summarize only messages after the side-conversation boundary. Inherited parent history before that boundary is context only and must not be summarized as side-conversation work.

On successful completion, the final assistant text from the summary turn is injected into the parent thread as a durable, model-visible assistant message headed:

```text
Side conversation summary
```

The side thread is discarded only after the parent injection succeeds.

## Failure Handling

If summarization fails, is interrupted, produces no assistant text, or the parent injection fails, the side conversation remains open and the TUI shows an error. The side thread must not be discarded on these failures.

## Validation

Validation should cover the close prompt default, Cancel/Esc/Ctrl+C behavior, Leave preserving the discard path, running-turn interrupt priority, unchanged composer and popup behavior, successful summary injection and discard, and failure cases that keep the side conversation open.
