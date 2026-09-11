# Bare Prompt Slash Command Warnings

## What it adds

This feature protects users from accidentally sending a plain prompt when they
likely meant to run a local slash command.

## Final behavior

- When the composer contains exactly `exit`, Codex shows:
  `Did you mean /exit?  enter to exit  esc to dismiss`
- When the composer contains exactly `resume`, Codex shows:
  `Did you mean /resume?  enter to resume  esc to dismiss`
- Pressing Enter while the warning is visible runs the matching slash command.
- Pressing Esc dismisses the warning for that exact draft. Pressing Enter after
  dismissal sends the literal prompt text.
- The warning is only for the exact lowercase draft text. Variants such as
  ` exit `, `Exit`, `/exit`, or `resume work` are ordinary prompts or commands
  and should not trigger this warning.
- The warning must not appear while another modal or popup owns the bottom pane.
- The warning takes precedence over lower-priority composer footer hints while
  it is visible.

## Why it matters

`exit` and `resume` are common command-like words. Sending them as normal model
prompts is almost always a typo when the local slash commands are available.
The confirmation keeps the fast path for intentional `/exit` and `/resume`
while still preserving a way to submit the literal words.

## Validation expectations

- Exact `exit` shows the warning and Enter exits Codex through the slash command
  behavior.
- Exact `resume` shows the warning and Enter opens the resume flow through the
  slash command behavior.
- Esc hides the warning without changing the draft text.
- After Esc, Enter submits the literal prompt text.
- Non-exact variants do not show the warning.
