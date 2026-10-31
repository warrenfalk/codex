# Copy Unsent Prompt Text

## What it adds

This feature adds a direct way to copy the current composer contents without
sending the prompt first.

## Final behavior

- `Ctrl+I` copies the current composer input from the main TUI view.
- Copying uses the expanded composer text, so pending paste placeholders are
  resolved before the clipboard content is written.
- Copy results use the same temporary feedback area as selection copies and stay
  visible with clean scrollback enabled. They do not add history cells.
- Show one result when copying finishes, without a separate "Copying…" notice.
- Terminal-only copy requests are reported as unconfirmed. They and failed
  copies preserve clipboard contents supplied by an earlier native copy.
- If there is no current input, Codex reports that explicitly instead of
  writing an empty string to the clipboard.

## Why it matters

This gives the composer a symmetric copy path next to `Ctrl+O` for the last
assistant message. It is useful when a prompt needs to be moved into another
thread, editor, or tool without actually submitting it.

Original implementation commit: `b1ee02a21c` (`tui: Ctrl+I to copy un-sent prompt text`)
