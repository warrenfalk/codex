# codex_file_search

Fast fuzzy file search tool for Codex.

Uses <https://crates.io/crates/ignore> under the hood (which is what `ripgrep` uses) to traverse a directory while honoring `.gitignore` and then uses <https://crates.io/crates/nucleo-matcher> to fuzzy-match the user supplied `PATTERN` against the corpus. File-search results are also augmented with a non-recursive read of the path currently being typed, so explicit prompt file mentions can still discover ignored immediate children without crawling ignored subtrees.
