# Project Environment Loading

## Intent

Codex should automatically load a repository's project environment before it runs thread-scoped project commands. A repo that already uses `.envrc` should work the same way in Codex as it does in a terminal with direnv and nix-direnv enabled.

The immediate motivation is a Nix flake workflow where `.envrc` contains behavior such as:

```sh
use flake
dotenv_if_exists .env.local
```

With project environment loading, the agent should not need to know to prefix commands with `nix develop -c ...`, and it should not need separate logic to load `.env.local` when `.envrc` already describes that environment.

## V1 Scope

V1 is local-environment only. It applies when commands run in a cwd local to the app-server host. Remote environments are left unchanged.

V1 uses real direnv and nix-direnv behavior. Codex invokes `direnv export json` and consumes the environment exported by direnv. Codex must not synthesize a replacement for direnv, must not hand-roll `nix develop`, and must not parse flakes or dotenv files itself.

V1 trusts `.envrc` for this fork regardless of whether the user's normal direnv trust state would consider the file trusted. Codex must not mutate the user's normal direnv allow/deny state for this behavior.

## Preserved Storage Boundary

Codex must preserve nix-direnv's normal repo-local layout behavior:

- Codex runs real `direnv export json` and lets direnv/nix-direnv evaluate `.envrc`.
- Codex does not set `DIRENV_LAYOUT_DIR`.
- Codex does not otherwise redirect `direnv_layout_dir`.
- nix-direnv remains responsible for its usual repo-local `.direnv` profile and cache layout.
- Codex-owned direnv trust/config/cache state lives under `.direnv/codex/project-env/xdg/...`.
- Codex-owned XDG state is only for direnv trust/config bookkeeping, not for nix-direnv layout or flake profiles.

This keeps the expensive Nix dev-shell cache in the place users already expect, while keeping Codex's private trust state deleted with the checkout and normally covered by existing `.direnv` ignore rules.

## Trust State

For V1, Codex privately trusts `.envrc` by using a Codex-owned XDG environment for direnv capture:

- `XDG_CONFIG_HOME=<envrc-dir>/.direnv/codex/project-env/xdg/config`
- `XDG_DATA_HOME=<envrc-dir>/.direnv/codex/project-env/xdg/data`
- `XDG_CACHE_HOME=<envrc-dir>/.direnv/codex/project-env/xdg/cache`

Codex may also set `DIRENV_CONFIG` if needed, but `DIRENV_CONFIG` alone is not sufficient because direnv allow/deny state is stored under XDG data paths.

V2 should revisit this trust model. The preferred V2 behavior is to prompt the user and use direnv's own trust state instead of automatically allowing `.envrc` in Codex-owned state.

## Command Behavior

Project environment loading applies to thread-scoped project command execution:

- model shell tools, including unified exec and legacy shell command paths
- app-server `thread/shellCommand`

It does not apply to:

- standalone app-server `command/exec`
- `apply_patch` interception
- hook commands in V1
- app-server startup
- MCP setup
- shell snapshot capture
- skill loading
- file reads and other Codex control-plane helpers

The command API has an explicit project environment mode:

- `auto`: default; use project environment loading when applicable
- `bypass`: skip project environment loading for this command

If `.envrc` exists but loading fails, `auto` commands fail visibly before launching. Codex must not silently fall back to bare execution. The error should tell the agent or client to retry with `project_env: "bypass"` or `projectEnv: "bypass"` to inspect or repair `.envrc`, `flake.nix`, `flake.lock`, `.env.local`, or related inputs.

`bypass` is per-command only. There is no sticky thread bypass in V1. The persistent kill switch is config.

## Configuration

V1 uses one top-level kill switch:

```toml
disable_project_env = true
```

The default is false. The key participates in normal config merging, including project config. When it is true, project environment loading is completely disabled: no prewarm, no command waiting, no project-env build entries in `/ps`, and no automatic direnv capture.

## Environment Precedence

Direnv capture itself runs from the app-server process environment with controlled overrides for cwd and Codex-owned XDG state. It does not use each command's filtered `shell_environment_policy` output.

Final command launch applies environment layers in this order:

1. Start with the environment produced by `shell_environment_policy` inheritance, include, and exclude rules.
2. Overlay the captured direnv environment.
3. Overlay `shell_environment_policy.set`.
4. Overlay Codex runtime variables, sandbox/proxy variables, `CODEX_THREAD_ID`, unified-exec markers, and per-command environment additions.

For V1, `PATH` follows this general ordering. Future work may add special PATH handling if real workflows show that the generic overlay is not enough.

Codex should not proactively strip variables exported by direnv. If `direnv export json` includes `DIRENV_*` variables, V1 includes them in the captured environment.

## Discovery And Cwd Rules

Codex follows direnv's normal parent lookup semantics from the command cwd. If no `.envrc` is found for that cwd or its parents, project environment loading is a no-op and commands run as they do today.

For canonical thread status and prewarm, the cwd source of truth is the primary selected local turn environment's cwd. If there is no primary local environment, project environment loading is not applicable.

Command `workdir` is authoritative for command execution. A command whose `workdir` differs from the thread cwd uses the project environment for that command workdir, matching direnv terminal behavior.

A one-off command workdir build does not replace the canonical thread project-env status unless the thread cwd itself changes.

## Capture And Reuse

Codex captures the environment once for a valid watched-input fingerprint and reuses that captured env map for later command launches. It does not run `direnv exec` around every command.

Captured environment maps are in-memory only. Codex must not persist the full captured environment to disk because it may contain secrets. It may persist metadata, fingerprints, status, and capped error summaries if useful, but the environment values themselves are process-lifetime state.

Concurrent commands for the same cwd and selected `.envrc` use single-flight behavior. The first `auto` command starts or joins the build, and concurrent `auto` commands wait on the same result.

Codex may keep an in-memory content-addressed cache keyed by the watched-input fingerprint. If files are reverted to a previously captured state during the same app-server lifetime, Codex may reuse the cached environment without rerunning direnv. This reuse is opportunistic and must not persist the environment map to disk.

## Invalidation

Invalidation is based on direnv's watched inputs, not only `.envrc`.

After capture, Codex reads direnv's watched paths and computes a Codex-owned fingerprint. The fingerprint should include:

- selected `.envrc` path
- `.envrc` content hash
- sorted watched paths
- existence, file type, size, and mtime for each watched path
- content hashes for small important files such as `.envrc`, `flake.nix`, `flake.lock`, and `.env.local`
- resolved direnv path and direnv version
- fingerprint schema version

Codex should not content-hash every watched path by default. Some `.envrc` files watch large directories or generated paths, and project environment loading must not turn every command into a hidden filesystem scan.

When watched inputs change for a cwd, Codex marks that cwd as `building` and discards the old active environment immediately. Commands must wait for the current environment. They must not continue using the obsolete active env while a rebuild is pending.

If the new fingerprint already exists in the in-memory environment cache, Codex can promote it to ready immediately.

## Prewarm And Waiting

Codex starts project environment loading opportunistically when a local thread is created, resumed, or its canonical cwd changes. Thread creation and resume do not block on Nix or direnv.

Commands enforce availability. An `auto` command waits for the project environment if a build is running, starts a build if needed, or fails if loading fails. A `bypass` command skips the wait and runs immediately using existing Codex environment behavior.

Command `timeout_ms` or equivalent command runtime limits apply only after the command process launches. Project environment build time is separate and does not consume the command runtime timeout.

Project environment builds have visible cancellation and may also have a generous watchdog as a backstop. The watchdog should not be the primary control surface.

## Status

Project environment status is separate from thread lifecycle status. Building a project environment must not make `ThreadStatus` active.

The app-server exposes ordinary v2 status API, not experimental-gated in V1:

- `thread/projectEnv/read`
- `thread/projectEnv/statusChanged`

`thread/projectEnv/read` returns status for the thread's canonical local cwd only. It is not a cache introspection API for every command workdir touched in the thread.

Status states:

- `disabled`: feature disabled by config
- `none`: feature enabled and local cwd exists, but no `.envrc` was found
- `building`: `.envrc` exists and capture is running
- `ready`: captured environment is available for the current fingerprint
- `failed`: `.envrc` exists, but capture failed or required tooling is missing

Status payloads should include bounded, UI-useful context such as cwd, `.envrc` path, short message, update timestamp, and watched-input count. Full direnv/Nix diagnostics belong in logs; command errors get capped diagnostics only when loading fails or is stopped.

Successful project environment loading should not inject direnv or Nix output into command output.

## `/ps` And `/stop`

Active project environment builds are visible in `/ps` and cancellable by `/stop`.

They are not normal shell sessions, but `/ps` should show enough information to explain that a project environment build is running, including cwd and command shape such as `direnv export json`.

`/stop` cancels active project environment builds as well as background terminals. Waiting commands fail before launch with a clear error:

```text
project environment loading was stopped before the command could run. Retry the command, or run with project_env: "bypass" to inspect or repair the project environment.
```

If a command process has already launched, project environment cancellation no longer applies to that command. Existing command cancellation behavior controls the launched process.

A turn interrupt cancels that turn's wait on the environment build, but it does not necessarily cancel the shared build itself. `/stop` is the explicit user action that cancels the build process.

## Missing Tooling And Failure

If no `.envrc` is found, project environment loading is `none` and commands run normally.

If `.envrc` is found but `direnv`, nix-direnv support, or required project tooling is unavailable, `auto` commands fail visibly with a bypass hint. This is not treated as `none`.

V1 resolves `direnv` from `PATH`. It does not bundle or pin direnv. This lets the user's configured direnv/nix-direnv installation define project behavior.

## Platform

V1 is Unix-only. This fork does not need a Windows design for the initial version.

## Security And Sandbox Boundary

Project environment capture runs unsandboxed as the app-server user for V1. It is trusted project setup, not an agent command.

Capture ignores the agent command's sandbox and network approval policy. It should not require a separate approval prompt in V1.

Controls around this trusted setup are:

- cwd set to the target local cwd
- Codex-owned XDG trust/config/cache state under `.direnv/codex/project-env`
- capped diagnostics
- `/ps` visibility
- `/stop` cancellation
- `disable_project_env` kill switch
- explicit per-command `bypass`

## Validation Expectations

Validation should cover:

- no `.envrc` results in `none` and no behavior change for commands
- successful `.envrc` capture affects command environment
- `use flake` and `dotenv_if_exists .env.local` are honored through real direnv behavior
- load failure prevents `auto` command launch and includes bypass guidance
- `bypass` runs without project environment loading
- watched-file changes discard the active env and trigger rebuild
- reverted watched inputs can reuse an in-memory cached env
- concurrent commands share one in-flight build
- command workdir uses that workdir's project environment
- thread cwd status is not replaced by one-off command workdir builds
- `/ps` lists active project environment builds
- `/stop` cancels active builds and fails waiters before command launch
- turn interrupt cancels the waiter without requiring the shared build to stop
- `disable_project_env = true` disables prewarm, waiting, status builds, and command injection
- app-server `thread/projectEnv/read` and `thread/projectEnv/statusChanged` expose bounded status
- standalone `command/exec`, hooks, `apply_patch`, and internal helpers do not use project environment loading
