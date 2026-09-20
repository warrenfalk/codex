# Copy Prior Transcript Response

## What it adds

This feature lets the TUI copy the assistant response immediately before the
currently selected transcript backtrack prompt.

## Final behavior

- When the transcript backtrack overlay is active, pressing the normal copy
  action copies the assistant response that precedes the selected user prompt.
- The copied response is plain Markdown/text suitable for pasting into another
  prompt, note, or external editor.
- If no assistant response exists before the selected prompt, the TUI reports
  that there is no previous response to copy.
- The shortcut applies only while the backtrack overlay is active; normal copy
  behavior outside the overlay is unchanged.
- The backtrack overlay exposes the copy hint while a backtrack target is being
  previewed.

## Why it matters

When reviewing a prior turn before resuming from it, the matching assistant
response is often the context the user wants to preserve or compare. Copying it
from the overlay avoids manually selecting transcript text while keeping the
backtrack preview state intact.
