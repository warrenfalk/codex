# File Mentions Insert Code Spans

## What it adds

This feature makes prompt `@file` completion insert selected paths as Markdown
code spans.

## Final behavior

- Selecting a file from the TUI `@file` popup replaces the active `@token` with
  a backtick-wrapped path, such as `` `codex-rs/tui/src/app.rs` ``.
- The inserted code span remains plain composer text and is submitted exactly as
  shown.
- Paths containing spaces no longer need double quotes just to stay readable in
  the prompt.
- Paths containing backticks use a longer Markdown code fence so the inserted
  span stays valid.
- Image selections still follow the existing attachment path instead of
  inserting a text file reference.

## Why it matters

File mentions are usually meant as references for the model, not shell
arguments. Wrapping selected paths in code spans makes the prompt clearer and
keeps paths with spaces or punctuation visually grouped without requiring the
user to add Markdown formatting by hand.
