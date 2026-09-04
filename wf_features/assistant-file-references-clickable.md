# Clickable Assistant File References

## What it adds

This feature upgrades transcript rendering so assistant-produced file references
become actionable links instead of plain text.

## Final behavior

- Assistant markdown links and plain file mentions are resolved while transcript
  output is rendered.
- Relative paths, absolute paths, basename-only references, and line or column
  suffixes are all recognized.
- When a bare filename is ambiguous, Codex consults a file reference index so a
  unique local match can still become clickable.
- The resolved link target uses the configured file-opener URI scheme, so local
  tools can open the exact file location directly from transcript output or
  scrollback.

## Why it matters

Review output becomes much faster to use. Instead of copying a path out of the
transcript by hand, the user can jump directly from an assistant answer to the
referenced file.

Original implementation commit: `a0ee93032b` (`tui: make assistant file references clickable in transcripts`)
