# Copy Unsent Prompt Text

## What it adds

This feature adds a direct way to copy the current composer contents without
sending the prompt first.

## Final behavior

- `Ctrl+I` copies the current composer input from the main TUI view.
- Copying uses the expanded composer text, so pending paste placeholders are
  resolved before the clipboard content is written.
- Success and failure are reported in history cells, matching the existing copy
  feedback pattern used elsewhere in the TUI.
- If there is no current input, Codex reports that explicitly instead of
  writing an empty string to the clipboard.

## Why it matters

This gives the composer a symmetric copy path next to `Ctrl+O` for the last
assistant message. It is useful when a prompt needs to be moved into another
thread, editor, or tool without actually submitting it.

Original implementation commit: `b1ee02a21c` (`tui: Ctrl+I to copy un-sent prompt text`)
