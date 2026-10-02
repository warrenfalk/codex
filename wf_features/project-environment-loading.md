# Project Environment Loading

## Intent

Thread-scoped project commands should run inside the same repository environment
that a user gets from `direnv` in a terminal. A checkout with `.envrc` should not
force the agent to know to prefix commands with `nix develop -c`, nor should
Codex duplicate flake or dotenv parsing that already belongs to direnv.

## Behavior

Project environment loading applies only to local thread-scoped project command
execution:

- model shell tools, including unified exec and the legacy shell command path
- app-server `thread/shellCommand`

It does not apply to standalone app-server `command/exec`, hooks,
`apply_patch`, shell snapshot capture, app-server startup, MCP setup, skill
loading, file reads, or other Codex control-plane helpers.

For each applicable command, Codex resolves the command cwd and performs normal
direnv parent lookup for `.envrc`. If no `.envrc` is found, loading is a no-op
and the command runs as before.

When `.envrc` is found, Codex uses real `direnv` from `PATH` and consumes
`direnv export json`. It does not synthesize a replacement environment, does not
hand-roll `nix develop`, and does not parse flakes or dotenv files itself.

Codex privately trusts `.envrc` for this fork using Codex-owned XDG state under
`<envrc-dir>/.direnv/codex/project-env/xdg/...`. This must not mutate the user's
normal direnv allow/deny state.

Codex must preserve nix-direnv's normal repo-local layout behavior:

- do not set `DIRENV_LAYOUT_DIR`
- do not otherwise redirect `direnv_layout_dir`
- leave nix-direnv responsible for its usual repo-local `.direnv` profile and
  cache layout
- use Codex-owned XDG state only for direnv trust/config/cache bookkeeping

## Command Modes

Applicable command APIs expose an explicit project environment mode:

- `auto`: default; load the project environment when applicable
- `bypass`: skip project environment loading for this command

If `.envrc` exists but loading fails, an `auto` command fails visibly before
launching. Codex must not silently fall back to bare execution. The error should
include a concise hint to retry with `project_env: "bypass"` or
`projectEnv: "bypass"` to inspect or repair the project environment.

`bypass` is per-command only. There is no sticky thread bypass in V1.

## Configuration

The top-level kill switch is:

```toml
disable_project_env = true
```

The default is false. The key participates in normal config merging, including
project config. When true, project environment loading is completely disabled:
no prewarm, no command waiting, no project-env build entries in `/ps`, and no
automatic direnv capture.

## Environment Precedence

Direnv capture runs from the app-server process environment plus the target cwd
and Codex-owned XDG overrides. It does not use the command's filtered
`shell_environment_policy` environment.

Final command launch applies environment layers in this order:

1. `shell_environment_policy` inheritance, include, and exclude result
2. captured direnv environment
3. `shell_environment_policy.set`
4. Codex runtime variables, sandbox/proxy variables, `CODEX_THREAD_ID`,
   unified-exec markers, and per-command additions

Codex should not proactively strip variables exported by direnv. If
`direnv export json` includes `DIRENV_*` variables, V1 includes them in the
captured environment.

## Status And Cancellation

Project environment status is separate from thread lifecycle status. Building a
project environment must not make a thread active.

The app-server exposes ordinary v2 APIs:

- `thread/projectEnv/read`
- `thread/projectEnv/statusChanged`

`thread/projectEnv/read` reports status for the thread's canonical local cwd
only. It is not a cache introspection API for every command workdir touched in
the thread.

Status states are:

- `disabled`
- `none`
- `building`
- `ready`
- `failed`

Status payloads include bounded UI-useful context: thread id, status, cwd,
`.envrc` path, message, update timestamp, and watched-input count. Full direnv
or Nix diagnostics must not be exposed in notifications.

Active project environment builds are visible in `/ps` and cancellable by
`/stop`. They are not normal shell sessions, but `/ps` should show cwd and a
command shape such as `direnv export json`.

When `/stop` cancels an active project environment build, commands waiting for
that build fail before launch with a clear stopped message and bypass hint. A
turn interrupt cancels that turn's wait, but does not have to cancel the shared
build itself.

## Validation Expectations

Validation should cover no-`.envrc` no-op behavior, successful environment
injection, private XDG trust state, unchanged nix-direnv layout behavior, failed
capture blocking `auto`, `bypass` skipping capture, watched-input invalidation,
single-flight concurrent builds, command workdir selection, `/ps` listing,
`/stop` cancellation, waiter interruption, `disable_project_env`, bounded status
APIs, and confirmation that standalone `command/exec`, hooks, `apply_patch`, and
internal helpers are unchanged.
