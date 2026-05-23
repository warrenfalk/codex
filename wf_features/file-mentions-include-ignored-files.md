# File Mentions Include Path-Local Ignored Files

## What it adds

This feature makes prompt `@file` search expose ignored files and directories
only when the user is typing a path that reaches their containing directory.

## Final behavior

- Prompt `@file` completion still uses the fuzzy file-search session backed by
  the `ignore` walker and `nucleo` matcher for the recursive corpus.
- The recursive corpus still honors `.gitignore`, `.ignore`, git excludes, and
  global gitignore configuration.
- File search continues to include hidden entries and follow symlinked
  directories.
- Results are augmented with a non-recursive read of the directory implied by
  the active token:
  - `@file` adds immediate current-directory entries beginning with `file`.
  - `@file/` adds immediate entries inside `file/`.
  - `@file/nested` adds immediate entries inside `file/` beginning with
    `nested`.
- The augmentation includes ignored files and folders but does not crawl their
  descendants unless the user types further into that relative path.

## Why it matters

The prompt file mention feature is an explicit user action, not a background
source-control operation. If the user types enough of a local path to reach an
ignored directory, Codex should help complete the next path segment without
turning every ignored subtree into part of the global fuzzy-search corpus.
