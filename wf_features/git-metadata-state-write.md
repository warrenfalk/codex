# Git metadata state writes

## Intent

Let an explicitly trusted permission profile run ordinary stateful Git commands without granting a
repository-specific path for every checkout. The permission follows the active turn's repository,
including linked worktrees, so one profile works across repositories and cwd changes.

## Configuration

Opt in from a named permission profile by adding the symbolic filesystem entry:

```toml
[permissions.trusted-workspace.filesystem]
":git_metadata" = "write"
```

The entry is valid only with `write` access. It is ignored by older Codex versions as an unknown
symbolic path, so sharing the config with an older binary does not broaden that binary's access.

## Required behavior

- Resolve the repository independently for every turn on the selected executor. Do not persist an
  absolute repository path in user configuration.
- Enable the permission only when the selected named profile contains the opt-in and the repository
  worktree is already writable under that profile.
- Support ordinary repositories and standard linked worktrees created by `git worktree`.
- Treat an empty or structurally incomplete `.git` directory as a missing repository; do not grant
  Git metadata writes merely because the marker directory exists.
- Keep a linked worktree's `.git` pointer file read-only. Grant the current worktree's private Git
  directory separately from the shared common Git directory.
- Fail closed for missing repositories, read-only profiles, malformed pointers, symlinked Git
  metadata roots, non-standard external Git directories, and mismatched worktree back-pointers.
- Recompute the grant after cwd, environment, or permission-profile changes so access from a prior
  repository does not remain active.
- Missing protected controls may require temporary host filesystem entries while a sandbox is
  active. Those entries must be removed after normal completion, cancellation, timeout, or abrupt
  supervisor termination. A later sandbox startup must reclaim an entry attributable to an
  interrupted prior sandbox while preserving a pre-existing empty file or directory.

The writable state includes the index, HEAD and other operation state, refs, objects, reflogs, and
their lock files. This should support the common `add`, `commit`, `fetch`, `pull`, `push`, `merge`,
`rebase`, `cherry-pick`, `revert`, `reset`, `restore`, `switch`, `branch`, `tag`, and `stash` flows.

## Protected controls

The overlay keeps these repository controls read-only:

- repository-local config and config lock files;
- hooks;
- legacy branches and remotes definitions;
- repository info files and object alternates;
- submodule metadata administration;
- shared worktree administration, except for the active worktree's private mutable state;
- linked-worktree `commondir`, `gitdir`, and worktree-local config files.

Consequently, commands whose purpose is to change those controls can fail. Examples include local
`git config` writes, hook installation, `git worktree add/remove/prune`, submodule administration,
and some maintenance or server-info commands.

These carveouts are defense in depth, not a complete security boundary against hostile repository
mutation. Writable refs, objects, and operation state inherently permit repository corruption and
history replacement. Filesystem rename, link, and platform-specific edge cases can also undermine
path-based carveouts when an attacker deliberately tries to replace an enclosing Git directory.
Use this opt-in only for repositories where that remaining integrity risk is acceptable.

Pre-existing hooks and config remain readable and can still affect Git execution; the feature
prevents the agent from persistently editing the protected files but does not disable behavior the
repository already contained.

## Validation expectations

- In an ordinary repository, a sandboxed Git add and commit succeed while local config writes and
  hook creation fail.
- In a linked worktree, shared refs/objects and the active private index are writable while the
  `.git` pointer and private control pointers remain read-only.
- Missing optional controls such as a linked worktree's `config.worktree` do not prevent sandbox
  startup and cannot be created from inside the sandbox.
- Hard-killing the sandbox supervisor does not leave a synthetic config lock behind, and a later
  sandbox run reclaims synthetic leftovers from interruptions before cleanup supervision began.
- Empty control files or directories that predate sandbox setup are preserved.
- A read-only project, malformed or forged pointer, unrelated repository, or later cwd does not
  inherit the grant.
- An empty or structurally incomplete `.git` directory does not receive the grant or prevent a
  sandboxed command from starting.
- The same resolution behavior works through local and remote executor filesystems; unsupported
  foreign path conventions fail closed.
