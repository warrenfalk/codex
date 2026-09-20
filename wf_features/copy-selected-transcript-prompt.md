# Copy Selected Transcript Prompt

## What it adds

This feature lets the TUI copy the currently selected prompt while previewing a
transcript backtrack target.

## Final behavior

- When the transcript backtrack overlay is active, pressing `Ctrl+I` copies the
  selected user prompt to the clipboard.
- The shortcut only applies while the backtrack overlay is active; normal
  composer `Ctrl+I` behavior continues to copy the unsent composer input.
- A successful copy appends a status entry saying the selected prompt was copied.
- If there is no selected prompt, the TUI reports that there is no selected
  prompt to copy.
- The copied text is exactly the selected prompt text that would be restored if
  the user confirmed that backtrack target.

## Why it matters

Backtrack preview is useful for inspecting earlier prompts before resuming from
them. Copying the selected prompt gives the user a quick way to reuse, compare,
or edit that prompt without changing the active composer draft.
