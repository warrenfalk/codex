# TUI session directory filters

The resume and fork pickers offer distinct `Cwd`, `Repo`, and `All` directory
filters. Users can find a session in the current checkout without disabling
worktree features, or broaden discovery to related checkouts without including
unrelated projects.

- `Cwd` matches the session's recorded working directory against the current
  directory, with the existing path normalization. It never includes sibling
  worktrees solely because they belong to the same repository.
- `Repo` retains linked-worktree discovery: include the corresponding directory
  in the primary checkout and registered linked checkouts that share validated
  Git metadata. Preserve the relative subdirectory, so `checkout-a/src` includes
  `checkout-b/src`, but not sessions recorded at either checkout root. Separate
  clones sharing a remote are not the same repository for this filter.
- `All` removes the directory restriction. Search, session status, source, and
  provider restrictions continue to apply in every directory mode.

Offer `Repo` when worktree features are enabled and the current directory has a
locally accessible, recognized Git repository. Otherwise offer `Cwd` and `All`.
If no current-directory filter is available, offer only `All`. Do not interpret
remote filesystem paths as local repositories.

The resume picker defaults to `Cwd` whenever a directory is known, including when
opened during a session. The fork picker defaults to `Repo` when available,
otherwise `Cwd`. Both use `All` when no directory is known. An explicit `--all`
opens in `All`. The filter choice applies to the current picker; opening another
picker uses these defaults again.

The toolbar orders the choices `Cwd`, `Repo`, `All`. Right moves forward and left
moves backward, wrapping and skipping unavailable choices. Compact layouts show
the selected mode. Changing scope restarts listing and pagination while keeping
the search text and other controls. Every page, including a fallback from the
session index, must use the selected scope; stale responses from an earlier scope
must not replace the new results.

Sessions from other checkouts retain their recorded directory and the existing
resume/fork directory-selection behavior. Comfortable rows continue to show the
checkout path for sessions from another worktree. Older servers that accept only
one directory retain the existing fallback to the current directory.

`codex resume --last` resumes the most recently updated eligible session in `Cwd`,
even with worktree features enabled. It must not fall back to a sibling worktree
when the current directory has no sessions. `codex resume --last --all` retains
unrestricted directory lookup.

`codex resume --last-for-repo` skips the picker and finds the most recently updated
eligible session in `Repo`. The explicit option works even when worktree features
are disabled, and allows finding a session from another worktree before choosing
where to resume it. It preserves the same relative-subdirectory boundaries as the
picker. Without locally accessible Git metadata, it falls back to the available
current-directory filter; with no known directory, existing unrestricted lookup
applies. Do not inspect remote paths using the client's filesystem.

Both latest-session options accept a single positional prompt and honor provider
selection and `--include-non-interactive`. `--last-for-repo` conflicts with `--last`,
`--all`, and an explicit session plus a prompt. These scopes also apply when
discovery falls back from the session index to recorded session files. Existing
resume directory-selection behavior applies after discovery, so the user can
resume a session in the new worktree. `fork --last` and agents-dashboard grouping
retain their existing behavior.

Validation covers exact-Cwd isolation with worktrees enabled, Repo inclusion and
subdirectory boundaries, unrelated repositories, All, defaults and availability,
both navigation directions, search and pagination across scope changes, index
fallback, and resume/fork selection. Snapshot coverage must show the three choices
and their selected states and defaults in wide and compact layouts. Latest-session
validation must distinguish an older Cwd session, a newer sibling-worktree session,
and a still newer unrelated session with worktree features both enabled and
disabled, including an empty Cwd and index fallback.
