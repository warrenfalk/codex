# Update test environment

Use this with [the replay guide](UPDATE_INSTRUCTIONS.md). Establish one working
validation environment before replay tests, record it in the ledger, and reuse
it at checkpoints. This is a recipe for preparing and checking the environment,
not a replacement test runner. Rust tests still run through `just test`.

## Preflight

1. Check available RAM and free space on the Cargo target, temporary directory,
   repository, and Nix store filesystems. Resolve any target symlink and check
   environment/configuration overrides; these locations can use different disks.
2. Enter the pinned flake shell and verify the required tools, including Rust,
   `just`, `cargo-nextest`, formatters, and any native dependencies changed by the
   release. Record version output. Keep V8 archives/bindings and other external
   native artifacts rooted while needed; use the exact versions and build flags
   expected by the checkout, not paths copied blindly from a previous release.
3. Confirm the selected Cargo profile, feature set, package set, target directory,
   stack setting, and launcher. Choose memory-appropriate build parallelism and
   retain it unless measurements justify changing it.
4. Verify required helper binaries and resource paths, including the upstream
   baseline if one will be used. A test that returns early for a missing helper
   has not exercised the behavior.
5. Run small representative tests covering workspace-binary spawning, shell
   execution, and any affected sandbox or TLS fixtures. Inspect successful-test
   output as well as failures. Record test names, counts, and actual behavior.
   Repeat affected preflight checks after changing the launcher, before another
   broad suite.

Use the same recorded environment for the final workspace suite. If workspace
feature unification requires an explicit feature, record and test that choice.
For v0.155.1, the full suite needed `codex-v8-poc/sandbox` to match the sandbox
already enabled by code-mode runtime. This is a diagnostic hint for later
versions, not a permanent instruction to enable arbitrary features or all features.

## Cargo target and fixture storage

Use a persistent `codex-rs/target` directory by default, or retain the user's
chosen persistent `CARGO_TARGET_DIR`. Cargo build artifacts do not need to live
in `/tmp`. Do not restore an old temporary target symlink simply because a prior
replay used one. Keep upstream comparison artifacts in their own persistent
target directory.

After moving or unlinking a target, check Cargo configuration, inherited
`CARGO_TARGET_DIR`, and ignored launchers under `personal/`. A launcher can
silently redirect builds to the old path even after the symlink is gone. Update
its bind mounts and helper-binary paths to use the selected target. The existing
incremental-cleanup script follows `codex-rs/target`; an explicitly overridden
target needs its own correctly scoped cleanup.

Keep the target readable outside a test's deliberately denied `/tmp`. A private
test `/tmp` may be backed by a directory on persistent storage; it need not use
the host's `/tmp` filesystem. For fixtures that hard-link large executables,
keep the prepared binaries and fixture directories on the same filesystem and
visible through the same mount inside the test namespace. Matching devices
alone does not guarantee that a hard link across separate bind mounts works.
Probe a real hard link in that namespace, and refresh prepared binaries from
the current build after relinking. Preserve serialization of genuine package
copy tests; moving storage does not eliminate I/O contention.

Unlinking a target does not move its cache. An empty replacement will rebuild
on demand. Reusing the old cache is optional; migrate it only while no build
uses either location, and let Cargo recheck freshness after relocation. Do not
rerun the complete suite solely because the cache moved; repeat the affected
launcher and helper preflight before the next behavioral validation.

## Linux/Nix shell fixtures

NixOS tests may expect conventional `/bin` and `/usr/bin` paths. Reuse an existing
working private mount-namespace fixture when available; keep machine-specific
paths and generated files in ignored `personal/`. Preserve native device access
needed by the tests. Do not install symlinks globally or change application code
to compensate for the validation host.

The v0.155.1 fixture needed these paths:

- `/bin`: `bash`, `sh`, `ps`, `sleep`, `kill`, `rm`, `cat`, `echo`, `mkdir`.
- `/usr/bin`: `env`, `printenv`, `true`, `false`, `touch`, `git`, `awk`, `sed`,
  `wc`, `tr`, `sort`, `getconf`.
- A private temporary directory plus a readable, stable Cargo target location.
  Sandbox-denial tests must still be able to read their helper executables;
  mounting the target only inside a deliberately denied `/tmp` breaks that probe.
- A loader-compatible bundled Zsh. Check the checkout's actual bundled binary
  and interpreter. If a fixture copy needs patching, overlay the copy only in the
  test namespace; leave the shared downloaded cache intact.

An ordinary Nix Bash can start successfully but have `/no-such-path` as its
compiled-in default `PATH`. Tests that clear the environment then fail to find
basic tools. Use the pinned nixpkgs `bashInteractive.override { forFHSEnv = true; }`
for this fixture and provide Dash for `/bin/sh`. Verify tool discovery under the
same cleared environment the tests use, not merely in the populated dev shell.

When a private `/tmp` hides Nix's original temporary directory, clear inherited
`TMPDIR`, `TMP`, `TEMP`, `TEMPDIR`, and `NIX_BUILD_TOP` in the test launcher. Start
with a clean fixture directory; stray `.git` or `.agents` markers can change
repository-discovery tests. Clear `NO_COLOR` when exercising color behavior.
Make these changes in the validation launcher, not in product configuration.

## Certificate and helper inheritance

For v0.155.1, inherited custom-CA variables changed transport selection in TLS
tests. Clearing them only outside the Nix shell was insufficient: the test child
still inherited them. When this symptom is confirmed, apply the following runner
inside the selected shell/fixture, scoped to test execution:

```bash
env 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=/usr/bin/env -u SSL_CERT_FILE -u CODEX_CA_CERTIFICATE -u CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER' \
  just test <the recorded package, feature and filter arguments>
```

This example is for the recorded native x86_64 Linux configuration. Adapt the
target runner key for another target. Do not overwrite an existing runner needed
for cross compilation or remote execution; compose and verify the required behavior.
Keep certificate settings needed by Nix/dependency downloads outside this test
child adjustment. Tests of explicit custom-CA behavior need their own fixtures.

Both details of that runner matter: `/usr/bin/env` must be an absolute path, and
the runner variable must remove itself from each test child's environment.
Otherwise nested `assert_cmd`/workspace-helper lookup can resolve to the runner
instead of the intended binary. Verify those helper-spawning tests before a
full suite; a runner that fixes TLS tests can still break unrelated integration tests.

## Equivalent upstream comparisons

Use a separate worktree at the target tag and a separate Cargo target directory.
Record any necessary lockfile reconciliation separately from source changes.
Match toolchain, features, profile, shell/sandbox fixture, and relevant environment.
If package selection must differ, record and assess the resulting feature differences.

Build the helpers that the selected tests actually launch. In v0.155.1, a narrow
baseline selection did not produce the normal `codex-execve-wrapper` and `bwrap`
executables. The required builds were:

```bash
cargo build --manifest-path codex-rs/Cargo.toml \
  -p codex-shell-escalation --bin codex-execve-wrapper
cargo build --manifest-path codex-rs/Cargo.toml -p codex-bwrap --bin bwrap
```

Run these in the baseline environment and target, not the active replay target.
Verify the same `debug/codex-resources/bwrap -> ../bwrap` layout if that checkout's
resource lookup requires it. Successful helper discovery must be established
before treating an early return as a pass or a different crash as an upstream match.

Compare the same test and failure signature. Keep known failures tied to a
specific release and configuration; never carry an old failure list forward as
an automatic exclusion from the next update. Run only the baseline tests needed
to classify current failures.
