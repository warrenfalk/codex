# Manual Replay Instructions

This document describes how to replay the commits in `rust-vX..main` onto a newer base tag without using `git rebase`.

It is based on the learnings of several updates since rust-v0.115.0.

## Goal

Reapply a source range onto a new base by:

1. checking each source commit just enough to identify its type and scope
2. replaying it either by cherry-picking with `--no-commit` or by rerunning a recorded `!exec` command or by following `!instruct` command
3. keeping the fast path fast: if the replay applies cleanly and validates, accept it
4. resolving conflicts and compatibility drift in the smallest possible way when the fast path fails.
5. preserving the right commit message shape for that replay
6. validating the tree after that commit
7. committing only when that individual replay is healthy (ALWAYS preserve any `!exec` or `!instruct` command in the original commit)

This keeps history understandable and makes it clear which replayed commit introduced which fix.

The exception is an intentional `[bug test]` / `[bug fix]` pair. A `[bug test]`
commit may be committed with a known failing validation only when the
immediately following `[bug fix]` commit is expected to make that same
validation pass.

The core rule is:

- A source commit is evidence of purpose, not sacred patch text.
- Your default move is still the fast path: try to apply it first.
- If it applies without conflict and the right validation passes, keep it and move on.
- Only when the replay conflicts, fails validation, or may already be obviated upstream do you slow down and reconstruct the commit's intent against the newer architecture.

## Ground Rules

- Do not use `git rebase`.
- Replay one source commit at a time. Use `git cherry-pick --no-commit` for normal commits.
- If a source commit message begins with `!exec `, do not cherry-pick it. Verify the working tree is clean, run the exact command after `!exec `, then `git add -A` and commit with the exact same `!exec ...` message.
- Inspect each source commit with `git show` before replaying it, but keep that inspection cheap on the fast path: identify whether it is a normal commit, `!exec`, or `!instruct`, and what area it touches.
- For a normal commit, attempt the cherry-pick first. If it applies cleanly and the validation for that commit passes, assume the commit is still needed and the old implementation is acceptable on the new base. Do not do extra archaeology just to prove that again.
- None of our replayed commits should add new build warnings. Treat any new warning like an error: find the source commit that introduced it, fix it in that commit's replay, and do not move on while the warning remains.
- Treat commit subjects beginning with `[bug test]` and `[bug fix]` as an intentional probe/fix pair for a branch-local bug. Replay the `[bug test]` first and run its targeted validation before replaying the matching `[bug fix]`. Keep the `[bug fix]` only if the test still fails on the newer base without it.
- Escalate to intent-level analysis only when one of these is true:
  - the cherry-pick conflicts
  - the replay validates incorrectly
  - you have reason to believe upstream already obviated the commit
- When you do need intent-level analysis, see if the commit adds a document to `wf_features`. If so, treat this as the commit's purpose. Otherwise treat the original patch as the old realization of the commit's purpose on the old base. Your job is to remake that purpose on the new base, not to preserve the exact old patch shape.
- Check that the commit message still matches what the replayed commit is accomplishing. Adjust the message if the replay required meaningful compatibility work or if the old wording no longer describes the new implementation accurately.
- If the replay required extra compatibility work for the new base, add this to the commit message.
- When the newer base has already implemented the same feature or workflow, prefer the upstream name, abstraction, and call pattern over preserving an older local variant in parallel.
- If our branch adds capabilities beyond upstream's version of the feature, graft those capabilities onto the upstream model with the smallest viable change.
- You may skip a source commit only when the newer base has genuinely obviated that commit's intent. "The old patch no longer applies cleanly" is not enough. "An earlier validation failed for a different reason before we reached the condition this commit fixes" is also not enough. In that case, fix and commit the earlier failure first, rerun the deciding validation, and only then decide whether the original commit is still needed.
- Preserve both the upstream path and the older local path only when the branch-only behavior cannot be incorporated cleanly into the upstream model without disproportionate churn or a real regression. If you keep both, say why in the replay note.
- Resolve conflicts by intent, not by mechanically forcing the old patch onto the new base. Read what the source commit was trying to accomplish, read the newer upstream code around the conflict, and then re-apply that behavior in the newer structure. For example, if upstream has replaced an older direct constructor call with a newer builder or helper flow, preserve that newer flow and thread the branch-specific behavior through it instead of reintroducing the old call pattern.
- When one replayed commit teaches you about an upstream refactor, assume nearby commits may hit the same drift. Record recurring patterns in the replay ledger and reuse them instead of rediscovering them commit by commit. Common recurring patterns include test modules being reorganized, constructor/call flows being replaced by newer abstractions, and code or assets moving between modules, crates, or directories.
- For this manual replay/update workflow, do not run repo-level maintenance or validation steps from `AGENTS.md` such as `just argument-comment-lint`, `just bazel-lock-update`, `just bazel-lock-check`, `just bazel-test`, or ad hoc `bazel build` / `bazel test` commands unless the source commit itself is explicitly being replayed via `!exec`.
- Prefer small compatibility fixes that preserve the original intent and avoid cascading churn into later commits.
- Keep `git show` and `git commit` as separate commands when approval boundaries matter.
- Avoid all nested shell command chains unless they are clearly worth it.

## Setup

- Find the most recent tag in our history that matches `rust-*`, e.g. `rust-v0.115.0`. This is our current version.
- Before beginning, tag the current commit replacing `rust-` with `wf-` eg: `wf-v0.115.0`.
- Ask GitHub for the latest Codex release tag:

  ```bash
  curl -fsSL https://api.github.com/repos/openai/codex/releases/latest | jq -r .tag_name
  ```

  The returned `rust-v*` tag, for example `rust-v0.116.0`, is the new version.
- Fetch the latest from `upstream` with `git fetch upstream`.
- Make sure the new-version tag and its target commit are present locally. The
  commit pointed to by the latest release tag is not always reachable from a
  fetched branch, so a normal `git fetch upstream` may not bring it in:

  ```bash
  git fetch upstream tag rust-v0.116.0
  ```

Start from the new base (replace `116` below with the new target version):

```bash
git switch -c manual-replay-v0.116.0 rust-v0.116.0
```

List the source commits you intend to replay:

```bash
git log --reverse --oneline rust-v0.115.0..main
```

Track progress explicitly. A simple checklist in a scratch buffer is enough.

Treat the `git log --reverse --oneline <old-base>..main` output as a fixed replay queue, not as a rough guide. Keep a simple replay ledger while you work, for example:

- source sha
- replay sha
- validation run
- replay note / known deviation
- recurring upstream drift to remember for later commits

This makes interruptions and handoffs much cheaper.

After any interruption, long conflict, or cluster of similar commits, audit the
ledger against the fixed queue before continuing. Do not rely on memory or the
latest replayed commit subject to decide what is next. Similar TUI or protocol
commit subjects are especially easy to skip accidentally.

Before spending time on the first commit, note any environment-only failures you already know about, such as:

- repo-level lint commands that currently fail because Bazel crashes during startup
- tests that are known to fail on this machine because `/bin/bash` or other expected host paths are absent

Record those once in the replay ledger so you do not keep re-discovering them.

If you are working in a sandboxed worktree, check early whether the `.git` metadata for this worktree is writable from your environment and whether commit hooks will run successfully. It is better to discover any needed escalation or `--no-verify` exception before the final commit than at the very end of a validated replay.

## Per-Commit Workflow

For each source commit `<sha>`:

### 1. Inspect the source commit

```bash
git show --stat --summary --format=medium <sha>
```

On the fast path, read only enough to answer:

- is this a normal commit, `!exec`, or `!instruct`?
- what subsystem does it touch?
- what validation will probably be needed if it replays cleanly?

Do a deeper read only if the replay later conflicts, fails validation, or might be obsolete upstream.

When you do need the deeper read, answer:

- what behavior is this trying to enable or protect?
- is it introducing API shape, tests, docs, generated schema, or only wiring?
- what parts are likely to conflict with the newer base?
- has the newer base already implemented most of this under a different name, flag, API, or orchestration model?

For a normal source commit, you do not need to pre-solve the whole replay before trying it. If the message type is clear and the target area is understood, attempt the cherry-pick.

If the source commit message begins with `!exec `, extract the command first and replay that commit by rerunning the command instead of cherry-picking its diff.

If the source commit begins with `!instruct`, use the instructions that follow as an addendum to this document

### 2. Replay the change

If the source commit subject begins with `[bug test]`, treat it as a bug probe:

1. Cherry-pick the test commit with `--no-commit`.
2. Run the narrow validation that exercises the new test before applying the paired `[bug fix]`.
3. If the validation passes on the newer base, upstream has already fixed the bug or made it irrelevant. Keep the `[bug test]` only if it still adds useful, non-duplicated coverage. Otherwise discard the staged test changes too. In both cases, skip the paired `[bug fix]` and record the upstream-obviated decision in the replay ledger.
4. If the validation fails for the expected bug, commit the `[bug test]` with that expected failure recorded, then replay the matching `[bug fix]` immediately afterward and validate that the same command now passes.
5. If the validation fails for an unrelated compile error, fixture drift, or changed test harness, fix that replay drift first. Do not decide whether to keep the `[bug fix]` until the `[bug test]` is actually probing the intended behavior. If the test harness itself moved forward, adapt the probe to the current helper or stack model instead of preserving the old harness shape.

Do not replay a `[bug fix]` before proving that its paired `[bug test]` still
fails on the new base. When the pair is still needed, keep the two commits
adjacent and keep their `[bug test]` / `[bug fix]` prefixes intact so future
updates can repeat the same decision.

For any other normal source commit:

```bash
git cherry-pick --no-commit <sha>
```

If the cherry-pick applies cleanly, prefer to keep moving. Validate that replay. If the validation passes, commit it. That is the default fast path.

If there are conflicts:

- inspect the surrounding code, not just the marker block
- preserve both the source intent and the newer-base abstractions
- prefer the newer upstream call patterns when they already subsume the older approach
- if upstream already covers the feature, align to upstream first and then re-add only the branch-specific behavior that is still missing
- keep high-conflict files as stable as possible

When a conflict is large, do not reason only inside the conflict markers. Compare the file directly from both sides:

```bash
git show HEAD:<path>
git show <sha> -- <path>
```

That is usually faster than reading a giant mixed conflict block, especially in high-drift files.

If a conflict touches source plus generated artifacts, resolve the source intent first and then reconcile generated files afterward.

For schema-bearing changes, fix the Rust/API source first and then run the
repo generator that owns the output. Do not hand-edit generated schemas or
TypeScript fixtures except as a temporary diagnostic step. Common commands are
`just write-config-schema` for config TOML shape changes and
`just write-app-server-schema` for app-server protocol shape changes.

For a source commit whose message begins with `!exec `:

```bash
git status --short
<exact command after !exec >
```

`git status --short` must be empty before you run the command.

### 3. Check the staged tree carefully

Useful commands:

```bash
git status --short
git diff --cached --stat
git diff --cached -- <path>
```

Look for replay-specific holes that are easy to miss:

- new protocol fields that now require extra callers
- new enum variants or CLI subcommands that must be added to fork-local exhaustive maps
- tests that compile only if new enum imports or helper modules are also present
- stale snapshots or generated schema files that no longer match source changes
- snapshot files that introduce or preserve `assertion_line:` header metadata; treat that line as unstable noise and do not commit it forward
- assertions that accidentally depend on the live shell environment instead of controlled test data

When a snapshot test fails, do not "fix" it by accepting a `.snap.new` whose meaningful change is only `assertion_line:`. Review the rendered snapshot body first. If the body change is intentional, keep the body change while preserving the snapshot header policy: remove `assertion_line:` from a newly added snapshot before staging it, do not add `assertion_line:` to an existing snapshot that does not already have it, and do not change or remove an existing `assertion_line:` line unless the user explicitly asked for snapshot-header cleanup. If the diff is only `assertion_line:`, discard the snapshot change entirely instead of perpetuating later merge conflicts.

Expect replayed UI commits to cause snapshot churn occasionally. Treat that as data, not as a surprise. Review the snapshot body quickly, decide whether it matches the replayed intent, and then either accept it or reject it deliberately instead of letting snapshot uncertainty slow down the whole replay.

### 4. Make the smallest compatibility fixes

When the new base differs from the source branch, prefer localized fixes over broad rewrites.

On high-drift files, preserve the newer base architecture and graft the replayed behavior onto it. Do not resurrect older constructor shapes, helper layouts, or call patterns if the new base already has a cleaner abstraction that covers the same behavior.

When upstream adds a new API enum variant or CLI subcommand, search for
fork-local exhaustive matches that classify those variants. Common examples are
helpers that map subcommands to user-facing names, decide which root flags are
allowed for each subcommand, convert protocol enums into UI labels, or render
all known `ThreadItem` variants. Add the new arm deliberately instead of using a
wildcard. A focused test can pass while the final workspace build still catches
one of these missing arms.

When the newer base has already subsumed the source feature, use this decision order:

1. keep the upstream shape and drop the older local duplicate
2. if the branch had extra capability, add only that incremental behavior on top of upstream
3. preserve both paths only when there is a concrete branch-only requirement that does not fit the upstream model cleanly

For example, if upstream renamed a connected-mode flag from `--connect` to `--remote`, replay the branch's remote-session behavior onto `--remote` unless there is a strong reason to keep both spellings.

Good examples from the `v0.116.0` replay:

- adding a newly required `execution_context: None` to an older `TurnStartParams` caller
- keeping both newer and incoming fixture fields in `SessionConfiguration` test setup
- importing a newly referenced enum in a test module
- adjusting a test helper to use controlled input instead of `std::env::vars()`

Bad pattern:

- “cleaning up” surrounding code that is unrelated to the replayed commit

When deciding whether a source commit can be dropped entirely, use a stricter standard:

1. identify the commit's actual intent
2. confirm that the newer base already provides that behavior, or makes it unnecessary
3. confirm that no branch-only behavior still needs to be reintroduced
4. if an unrelated failure currently blocks the deciding validation, fix that first and then re-check before skipping the original commit

Examples:

- A vendoring-hash `!instruct` commit is not obviated just because `nix build` currently fails earlier on a missing prerequisite package. Fix the prerequisite failure first, commit that fix, and only then decide whether the vendoring-hash issue still exists upstream.
- A connected-TUI replay commit may be obviated if upstream already implemented the same reconnect behavior, but only after you verify that the branch did not also add an extra cwd override or test expectation that upstream still lacks.

If the replayed commit changes generated outputs or vendored fixtures, regenerate them early rather than debugging stale failures first. Typical examples include:

- JSON or TypeScript schema output
- OpenRPC fixtures
- Bazel lockfiles after dependency changes
- snapshot files whose rendered output legitimately changed

### 5. Format after Rust edits

Run from the repo root through the flake dev shell:

```bash
nix develop -c just fmt
```

Do this after you finish the compatibility edits for that replayed commit.

The flake dev shell provides the formatter dependencies and sets `UV_CACHE_DIR`
to a writable workspace-local cache. It also provides `dotslash`, which `just
fmt` uses to run the repo's `tools/buildifier` wrapper for Bazel/Starlark files.
That formatter pass is not a Bazel build, test, lockfile, or
dependency-maintenance step. Do not compensate for formatter problems by running
Bazel maintenance commands; those are still out of scope unless the source
commit itself is an explicit `!exec` replay of one of those commands.

Review formatter diffs before staging. If the formatter rewrites unrelated
notebooks, generated SDK files, or the root `justfile`, revert that churn unless
the replayed commit specifically owns those files.

### 6. Validate the replayed commit

Run the narrowest cargo validation that actually covers the touched behavior.

Read the compiler output, not only the exit status. Pre-existing warnings from the upstream base can remain, but a replayed commit must not introduce any new warnings in touched crates. If a validation command emits a new warning, treat that command as failed and fix the warning before committing.

Examples:

- TUI-only changes:

```bash
cargo test -p codex-tui
```

- protocol changes:

```bash
cargo test -p codex-app-server-protocol
```

- app-server turn handling:

```bash
cargo test -p codex-app-server turn_start
```

- core execution-environment changes:

```bash
cargo test -p codex-core execution_context
```

- pure docs-only changes with no code, schema, snapshot, or generated artifact updates:

No Cargo validation is required.

- docs that accompany replayed code behavior:

Use the validation for the code change. Do not add an extra `cargo build` just because docs were updated in the same replayed commit.

- flake or Nix-related changes that do not modify the repo-root package build logic, vendoring, source patching, or other behavior that `nix build` is meant to prove:

```bash
cargo build
```

- changes to the relevant Nix packaging sections of `flake.nix`, `codex-rs/default.nix`, vendoring hashes, source patching, or other inputs that specifically affect whether repo-root `nix build` succeeds:

```bash
nix build
```

`nix build` takes a long time. Do not use it as the default smoke test for every replayed commit. Run it when the replayed commit actually changes the Nix packaging/build path, or when a `!instruct` commit explicitly tells you to decide whether a Nix-specific failure has been fixed upstream. In other cases, prefer the narrowest targeted cargo test or `cargo build`.

If the broader targeted test fails on one obvious snapshot or one focused test case, switch to the narrowest reproducer until you have fixed that problem, then rerun the broader target once at the end. Do not keep paying the cost of a full crate test loop while debugging a single snapshot body.

Do not run repo-level lint, Bazel, or lock-maintenance steps from `AGENTS.md`, such as
`just argument-comment-lint`, `just bazel-lock-update`, `just bazel-lock-check`, `just bazel-test`, or ad hoc `bazel build` / `bazel test`, as part of this manual replay/update workflow. They are intentionally out of scope for replay validation unless the source commit itself is an explicit `!exec` replay of one of those commands.

### 7. Commit the replayed change

For a replayed `!exec ` commit:

```bash
git add -A
git commit -m '!exec <exact command>'
```

Keep the message exactly aligned with the source commit so the regeneration step is explicit.

For a normal replayed commit, use a subject that states the user-facing or architectural purpose.

Good:

- `connected tui: preserve local execution context in remote turns`
- `docs: snapshot connected TUI capabilities and gaps`

Avoid subjects that just restate the source branch shorthand:

- `STASH wf_docs/CURRENT_STATE_CONNECTED_TUI.md`
- `fix tests`
- `wire new field`

When the replay needed meaningful compatibility work, add a body like:

```text
Replay note: v0.116.0 needed small compatibility follow-ups in ...
```

When the replay decision or compatibility work depended on a specific command succeeding, add a `Validation:` line with the exact command you ran. This is especially useful for:

- commits you kept or skipped because upstream had or had not already obviated the change
- `!instruct` follow-ups that depended on a specific `cargo build` or `nix build` result
- replays whose compatibility fixes were guided by one targeted test
- `[bug test]` / `[bug fix]` pairs where the test result decided whether the local fix was still needed

Examples:

```text
Replay note: v0.120.0 already had the newer app-server session builder, so this replay only threaded the branch-specific cwd override through the upstream path.
Validation: cargo test -p codex-tui connected_app_server
```

```text
Replay note: v0.120.0 still needed the local Nix prerequisite fix before the vendoring-hash check could be evaluated.
Validation: XDG_CACHE_HOME="$PWD/.cache" nix build
```

For pure docs-only commits, you may omit `Validation:` entirely or say so explicitly:

```text
Validation: not run (docs-only change)
```

For a kept `[bug test]` commit whose expected validation fails before the
paired fix, make that explicit instead of pretending the commit is healthy by
itself:

```text
Validation: cargo test -p codex-app-server-protocol thread_rollback_drops_user_boundaries_inside_explicit_turn
Expected failure: this bug probe fails without the paired [bug fix] commit.
```

The paired `[bug fix]` commit must include a `Validation:` line showing that
the same command now passes. If the `[bug test]` passes before the fix, skip the
`[bug fix]` and record that decision in the replay ledger.

In both cases, commit only after the validation for that replayed commit passes.
For `[bug test]` / `[bug fix]` pairs, this rule applies to the pair as a unit:
do not leave a failing `[bug test]` without its passing `[bug fix]`.

## What To Validate First

Use the changed area to choose the first test:

- `codex-rs/tui`: `cargo test -p codex-tui`
- `codex-rs/app-server-protocol`: `cargo test -p codex-app-server-protocol`
- `codex-rs/app-server`: `cargo test -p codex-app-server <targeted test>`
- `codex-rs/core`: `cargo test -p codex-core <targeted test>`
- pure docs-only changes: no Cargo validation required
- Nix-related changes outside the repo-root package build path: `cargo build`
- changes to the Nix packaging/build path itself: `nix build` and expect it to take a while

Do not jump straight to a full workspace test unless the change actually warrants it and you have approval if repo instructions require approval first.

## Lessons From The `v0.116.0` Replay

### 1. The source diff is not enough

Several commits replayed cleanly but still failed later because the newer base had evolved callers, fixtures, or test modules around the same APIs.

Always check nearby callers and test scaffolding, not only the staged diff.

### 2. Environment-sensitive tests need controlled inputs

One replayed execution-context test failed because the original test asserted against a map created from the real process environment. On the replay target, that environment was much larger than the test expected.

If a test is about merge or override behavior, feed it a controlled variable set instead of relying on `std::env::vars()`.

### 3. App-server and protocol changes often need multiple targeted tests

A single connected-TUI commit ended up needing:

- `codex-app-server-protocol`
- `codex-core execution_context`
- `codex-app-server turn_start`
- `codex-tui connected_app_server`

That was the right tradeoff because it caught integration drift early without requiring a full workspace suite after every replayed commit.

### 4. Long cargo runs can look stuck when they are not

Repeatedly restarting cargo while `rustc` is still compiling `codex-core` just wastes time.

Before killing a run, check whether it is doing real work:

```bash
ps -ef | rg '[r]ustc --crate-name codex_core|[c]argo test|[c]argo build'
```

If `rustc` is active and burning CPU, wait.

### 5. Generated files should stay coupled to source intent

Protocol commits often updated:

- Rust protocol types
- JSON schemas
- TypeScript schema output
- README examples

If the source commit was changing API shape, keep those generated artifacts with the same replayed commit instead of splitting them out arbitrarily.

### 6. Keep compatibility fixes local to the replayed commit

Do not postpone obvious replay breakage to an unrelated later commit.

If a replayed commit introduces a field, request type, or behavior that breaks immediately on the new base, fix that in the replayed commit so the history still explains itself.

### 7. Keep an explicit replay queue

Do not infer “what is next” from memory or recent history once the replay is underway.

Use the `git log --reverse --oneline <old-base>..main` output as the source of truth, and keep a scratch ledger mapping source SHAs to replay SHAs and validation notes. This saves time after interruptions and makes it obvious whether the replay is actually complete.

### 8. Compare sides directly when conflicts get large

For a large or high-drift conflict, use:

```bash
git show HEAD:<path>
git show <sha> -- <path>
```

This is usually faster than reading a huge mixed conflict block, and it helps separate:

- what the new base already changed
- what the source commit actually meant to add

Do not stop there. After you understand both sides, re-apply the source commit's intent in the new upstream structure instead of trying to preserve the old patch shape byte-for-byte.

### 9. UI replay churn often shows up as snapshot updates

When a replayed TUI or UI-adjacent commit fails only on a snapshot, that often means the code is already correct and the base simply renders the same intent slightly differently.

Review the snapshot body quickly, accept the intentional change, and continue. Do not treat every snapshot delta as a sign that the replay logic is wrong.

### 10. Preserve the new base architecture

When replaying onto a newer base, the fastest path is often:

- keep the newer constructor or orchestration layout
- identify the specific behavior the source commit was adding
- thread that behavior into the newer structure with the smallest possible change

Trying to force the older file shape back into the newer base usually creates extra churn and more conflicts in later commits.

### 11. Let upstream subsume local duplicates when it can

If the new base already implements the feature you were about to replay, do not preserve an older local alias, flag, or codepath by default just because it existed on the source branch.

Prefer this order:

- keep the upstream spelling and structure
- port only the branch-specific behavior that upstream still lacks
- keep both only when the branch-only behavior does not fit upstream cleanly

This keeps the replay converging toward upstream instead of accumulating parallel ways to do the same thing.

### 12. The fast path is "apply, validate, keep"

Do not turn every replay into a manual redesign exercise.

For a normal commit:

1. inspect it just enough to know what kind of replay it is
2. try the cherry-pick
3. if it applies cleanly, run the right validation
4. if validation passes, keep it and move on

That is good enough. Assume the commit is still needed and adequately coded unless the replay gives you evidence to the contrary.

### 13. Skip a commit only when its intent is truly obsolete

The correct question is not "can I make the patch disappear?" It is "has upstream already made this commit's behavior unnecessary?"

Do not skip a commit merely because:

- the old patch conflicts badly
- the file moved
- the deciding validation currently fails earlier for some unrelated reason

Do skip a commit when you can show that upstream already covers the behavior or makes it irrelevant, and no branch-only behavior remains to port forward.

### 14. Reuse conflict knowledge across later commits

One upstream refactor often produces a whole family of similar replay issues. When you notice one, write it down and expect it again.

Common examples:

- a test file was split into helper modules, so later commits touching the same behavior need the same test-location adjustment
- an older constructor/call sequence was replaced by a newer builder or orchestration layer, so later commits need the same newer flow preserved
- files, metadata, or generated assets moved to a different module, crate, or directory, so later commits must follow the new location rather than restoring the old layout

That short note in the ledger is cheaper than solving the same structural mismatch three times.

### 15. Record environment-only failures once

Some failures are about the replayed code. Others are just properties of the current machine or sandbox.

When you confirm an environment-only failure, record it once and stop re-learning it every few commits.

### 16. Anticipate hook and worktree constraints

If the repo uses commit hooks that run heavy lint or Bazel steps, or if the worktree stores Git metadata outside your writable sandbox, plan for that before the final commit.

The replay itself may validate cleanly while the default `git commit` path still fails for reasons that are unrelated to the replayed patch. Knowing that early helps you choose the right commit path without losing momentum.

## Suggested Command Sequence

For one commit:

```bash
git show --stat --summary --format=medium <sha>
git cherry-pick --no-commit <sha>
git status --short
git diff --cached --stat
git diff --cached -- <interesting paths>
cd codex-rs
just fmt
cargo test -p <targeted-crate> <targeted-test>
cd ..
git commit -m "<intent subject>" -m "<optional replay note>"
```

For a `!exec ` commit:

```bash
git show --stat --summary --format=medium <sha>
git status --short
<exact command after !exec >
git status --short
git add -A
git commit -m '!exec <exact command>'
```

Repeat until `git log --reverse --oneline <old-base>..main` has been fully replayed.

## Finish Criteria

The replay is complete when:

- every commit in the source range has been replayed
- each replayed commit has the correct message shape for its type
- replay-specific compatibility fixes were folded into the appropriate replayed commits
- every kept `[bug test]` commit either passes on the new base by itself or is immediately followed by the `[bug fix]` commit whose validation makes it pass
- the branch is clean
- the final targeted validation passes
- a final Rust workspace `cargo build` passes after the full replay, even if all
  focused tests passed earlier
- repo-root `nix build` passes when the update instructions, repository
  instructions, or changed files require it

Confirm at the end:

```bash
git status --short
git log --oneline --decorate -n 10
```
