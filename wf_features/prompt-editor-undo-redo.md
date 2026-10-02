# Prompt editor undo and redo

## What it adds

Every editable prompt field keeps a private, bounded undo and redo history for the draft being
edited. The default non-Vim shortcuts are `Alt+U` for undo and `Alt+E` for redo. `Ctrl+Z` remains
available to suspend the process. In Vim mode, normal-mode `u` undoes and `Ctrl+R` redoes.
The bindings are configurable as `editor.undo`, `editor.redo`, `vim_normal.undo`, and
`vim_normal.redo` through the TUI keymap.

## Final behavior

- An undo point restores the complete draft: visible text, cursor position, shell mode, text
  elements, local and remote image attachments, mention targets, and pending large-paste payloads.
- Ordinary typing is grouped until cursor movement or another editing action ends the group. A
  period, comma, semicolon, colon, question mark, exclamation point, or newline is included in the
  current group and then ends it.
- Repeated backward deletes form one group. Repeated forward deletes form a separate group.
- Pasting, accepting a completion, changing attachments, word or line deletion, clearing with
  `Ctrl+C`, and a successful external-editor round trip are atomic edits.
- A new edit after undo discards redo history. Cursor movement alone does not create an undo point
  or discard redo history.
- Vim insert sessions, including a normal-mode change that enters insert mode, form Vim-style undo
  units. Normal-mode edits form individual units. Vim cursor movement ends the current unit.
- Successful submission or queuing clears both stacks. Undo never restores a prompt that was sent.
- Up/Down history recall clears both stacks and treats the recalled draft as a new baseline.
- Reverse history search previews do not affect either stack. Escape restores the exact draft from
  before search; Enter accepts a confirmed match as a new baseline.
- Vim-normal `Ctrl+R` takes precedence over reverse history search. Reverse history search retains
  its configured binding outside Vim normal mode and can be remapped for use there.
- Programmatic draft replacement establishes a new baseline. Each prompt field owns its own
  history, so embedded request, elicitation, and reusable composer inputs do not share undo state.
- Each direction retains at most 100 snapshots and approximately 16 MiB. Oldest states are
  discarded first; a newest snapshot is retained even when it alone exceeds the memory budget.

## Validation expectations

- Exercise default and remapped bindings in both editor modes, including the `Ctrl+R` precedence in
  Vim normal mode and continued `Ctrl+Z` job-control behavior.
- Cover typing boundaries, cursor boundaries, backward and forward deletion groups, divergent edits,
  and full-draft restoration.
- Cover `Ctrl+C`, external-editor edits, successful submit/queue, Up/Down recall, and both cancel and
  accept paths for reverse history search.
- Verify the behavior through the shared prompt editor so the main composer and embedded prompt
  fields receive the same semantics.
