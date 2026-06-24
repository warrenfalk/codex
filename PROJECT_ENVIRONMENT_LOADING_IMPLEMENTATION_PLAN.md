# Project Environment Loading Implementation Plan

## Goal

Implement V1 project environment loading as specified in `wf_docs/project-environment-loading.md`.

The first implementation should be narrow: local thread-scoped command execution, real direnv/nix-direnv evaluation, explicit bypass, bounded status, and visible cancellation. Avoid broad refactors and keep app-server protocol changes small.

Before landing an implementation commit for the fork-local feature, make sure the durable preserved-behavior documentation requirement is satisfied. If `wf_docs/project-environment-loading.md` remains the planning/spec location, add or mirror the final observable behavior in `wf_features/project-environment-loading.md` as part of the implementation commit.

## Stage 1: Project Env Crate

Create a new Rust crate under `codex-rs/project-env` instead of adding the subsystem directly to `codex-core`.

Responsibilities:

- discover the applicable `.envrc` for a local cwd using direnv-compatible parent lookup
- run private `direnv allow` and `direnv export json`
- set Codex-owned XDG state under `<envrc-dir>/.direnv/codex/project-env/xdg/...`
- avoid setting `DIRENV_LAYOUT_DIR`
- decode the exported environment map
- collect watched inputs from direnv after capture
- compute Codex-owned watched-input fingerprints
- maintain in-memory ready/building/failed state
- single-flight concurrent builds
- cancel active builds
- expose bounded status and diagnostics

Suggested public types:

- `ProjectEnvManager`
- `ProjectEnvConfig`
- `ProjectEnvMode` with `Auto` and `Bypass`
- `ProjectEnvStatus`
- `ProjectEnvState` with `Disabled`, `None`, `Building`, `Ready`, `Failed`
- `ProjectEnvOverlay`
- `ProjectEnvBuildInfo` for `/ps`
- structured errors for failed, stopped, disabled, and not applicable cases

Keep the crate API centered on one operation:

```rust
async fn environment_for_command(cwd, mode, cancellation) -> Result<Option<ProjectEnvOverlay>, ProjectEnvError>
```

`None` means no `.envrc` or disabled/not applicable behavior where the caller should run normally. Failure means an `.envrc` exists but loading did not succeed and `auto` command launch should stop.

## Stage 2: Config Wiring

Add top-level config:

```toml
disable_project_env = true
```

Default is false. Thread/project config merging should use normal existing config behavior.

Update config schema generation as required by the repo rules:

```bash
just write-config-schema
```

## Stage 3: Session Service Integration

Add a session-owned `ProjectEnvManager` beside the existing unified exec manager in `SessionServices`.

Initialize it from:

- effective config, including `disable_project_env`
- app-server process environment snapshot for capture
- session/runtime handle needed for async process management

On thread spawn/resume and canonical local cwd changes, opportunistically prewarm the primary selected local turn environment cwd. Do not block thread creation or resume.

Do not include project env in shell snapshot capture. Shell snapshot remains separate.

## Stage 4: Command Tool API

Add a command parameter to model command surfaces:

- `project_env` on `exec_command`
- `project_env` on legacy `shell_command` while it exists

Allowed values:

- `auto`
- `bypass`

Default is `auto`.

For unified exec, resolve the command cwd first, then ask `ProjectEnvManager` for an overlay unless mode is `bypass` or the environment is not local. Merge the overlay according to the spec before launching the process.

For legacy shell command, apply the same mode and environment layering in the classic exec path.

If project env loading fails or is stopped, return a model-visible tool error before launching the command. Include a concise bypass hint.

## Stage 5: App-Server Thread Shell Command

Add `projectEnv?: "auto" | "bypass"` to `ThreadShellCommandParams`, defaulting to `auto`.

Thread shell command should use project environment loading because it is thread-scoped project command execution. It should fail visibly on broken project env and allow explicit bypass.

Do not change standalone `command/exec` in V1.

Regenerate app-server protocol schema and TypeScript fixtures:

```bash
just write-app-server-schema
```

Validate protocol changes:

```bash
just test -p codex-app-server-protocol
```

## Stage 6: Status API

Add ordinary v2 app-server API, not experimental-gated:

- `thread/projectEnv/read`
- `thread/projectEnv/statusChanged`

`thread/projectEnv/read` reports only the canonical thread cwd status. It should not list every command workdir touched by the cache.

Keep status separate from `ThreadStatus`. Project env building must not make a thread active.

Suggested payload:

- `threadId`
- `status`
- `cwd`
- `envrcPath`
- `message`
- `updatedAt`
- `watchedInputCount`

Use bounded diagnostics. Do not expose full Nix output in notifications.

## Stage 7: `/ps` And `/stop`

Extend the background process listing path to include active project environment builds.

The implementation can keep project-env builds separate from unified exec internally, but TUI `/ps` and app-server background process listing should show active builds in a way users can understand.

Extend `/stop` / clean background terminals behavior to cancel active project-env builds. Waiting commands should fail before launch with the stopped message from the spec.

Turn interrupt should cancel the command's wait, not necessarily the shared build. `/stop` is the explicit build cancellation control.

## Stage 8: Tests

Prefer integration tests for command behavior. Add crate-level unit tests only for fingerprinting and manager state transitions that are awkward to exercise end to end.

Targeted coverage:

- no `.envrc` no-op status and command behavior
- successful capture injects exported variables
- private XDG trust state is used under `.direnv/codex/project-env`
- nix-direnv layout is not redirected by Codex
- failed capture blocks `auto` and suggests bypass
- `bypass` skips capture and runs
- watched input changes invalidate active env immediately
- reverted fingerprint can reuse in-memory env
- concurrent commands single-flight one build
- command workdir determines project env for that command
- one-off command workdir build does not replace canonical thread status
- `/ps` lists active env builds
- `/stop` cancels env build and fails waiters
- turn interrupt cancels wait without requiring build cancellation
- `disable_project_env` disables all behavior
- `thread/projectEnv/read` and notification payloads are bounded and separate from `ThreadStatus`
- `thread/shellCommand` honors `projectEnv`
- `command/exec`, hooks, `apply_patch`, and internal helpers are unchanged

For test fixtures, use fake `direnv` scripts where possible to avoid requiring real Nix evaluation. Add one optional or narrowly-gated real-direnv smoke test only if it is stable in local CI.

## Stage 9: Validation Commands

After code changes:

```bash
just fmt
```

Run scoped tests for changed crates, likely:

```bash
just test -p codex-project-env
just test -p codex-core
just test -p codex-app-server-protocol
just test -p codex-app-server
```

If common/core/protocol behavior changes pass, ask before running the full suite:

```bash
just test
```

If `ConfigToml` changes, run:

```bash
just write-config-schema
```

If app-server protocol changes, run:

```bash
just write-app-server-schema
```

## Implementation Notes

Use `direnv` from `PATH` for V1. Capture runs under the app-server process environment plus controlled cwd and XDG overrides. Do not apply per-command `shell_environment_policy` to the capture process.

Final command environment layering is:

1. `shell_environment_policy` inherited/include/exclude result
2. captured direnv overlay
3. `shell_environment_policy.set`
4. Codex runtime, sandbox/proxy, thread, unified-exec, and per-command additions

Keep diagnostics capped at every app-server/model-visible boundary. Successful builds should be quiet.

Avoid adding implementation detail to the feature spec. Put code layout and staged work in this plan, and preserve observable behavior in the feature doc.
