# Connected TUI Environment

This document describes a v1 design for carrying client-derived execution
environment information from the connected TUI to the app-server.

The motivating problem is simple:

- In local TUI mode, launching Codex from inside `nix develop` means tool
  commands inherit that environment.
- In connected TUI mode, the app-server currently inherits only its own process
  environment. The client sends `cwd`, but not the environment associated with
  that `cwd`.

The goal of this design is to make connected TUI tool execution behave more
like local TUI, starting with Nix-based environments.

## Scope

This is intentionally a v1.

It does:

- define a thread-scoped `executionContext`
- have the client compute that context
- send it anywhere the client already sends `cwd` for thread state
- have the server remember it on the thread
- have the server apply it when spawning tool commands
- cache client-side Nix evaluation by `cwd`

It does not:

- try to preserve arbitrary interactive shell state
- try to reproduce `direnv`, `.env`, or other environment managers yet
- introduce per-client environments on the server
- try to infer a remote environment for a different machine

## High-level Model

The server should treat `executionContext` the same way it treats `cwd`:

- it is thread state
- the client supplies it
- the server stores it on the thread
- later tool execution uses the stored value

For v1, the wire format is intentionally generic and server-agnostic:

```json
{
  "executionContext": {
    "env": {
      "PATH": "...",
      "PKG_CONFIG_PATH": "...",
      "RUST_SRC_PATH": "..."
    }
  }
}
```

The server does not need to know that the client derived this from Nix.

## Why `env` only

For v1, `executionContext` should contain only an environment map:

- no shell script
- no sourceable activation text
- no command string
- no Nix-specific protocol type

This keeps the trust boundary narrow:

- the client may run a fixed local command to derive the environment
- the server accepts only structured data
- the server never executes client-provided shell code to derive context

## How the client derives `executionContext`

For now, "get the environment for a `cwd`" means:

1. Detect whether a relevant `flake.nix` exists for that `cwd`.
2. Check whether the `nix` command is available locally.
3. If both are true, run `nix print-dev-env --json`.
4. Parse the JSON and project it into `executionContext.env`.
5. Cache the result by `cwd`.

If any of those steps fail, the client sends no `executionContext`.

The exact "detect if there is a flake" rule is intentionally policy, not
protocol. The v1 implementation can be simple. The important point is that
whatever rule we choose is applied client-side and cached by `cwd`.

## Nix command details

The client runs a fixed local command, not an arbitrary command coming from the
server:

```bash
nix print-dev-env --json
```

This should be run in the selected `cwd`.

The client parses the JSON and extracts exported string variables into
`executionContext.env`.

For v1, the client should ignore anything that does not naturally map to a
plain process environment map, for example:

- shell functions
- shell-specific metadata
- values that are not plain strings

The goal is to make spawned commands see the right environment, not to recreate
an interactive shell.

## Same-machine assumption

This design is only meant for connected clients on the same machine as the
server.

If the client is on another machine, it should not send `executionContext`,
because a machine-local environment from one host is not meaningful on another
host.

That keeps the behavior easy to reason about:

- local client to local server: send `executionContext` when available
- remote client to remote server on another machine: omit it

## Request attachment points

Today, the connected TUI sends `cwd` on:

- `thread/start`
- `thread/resume`
- `thread/fork`
- `turn/start`

For v1, `executionContext` should be added to the same requests.

That gives these semantics:

- `thread/start`: sets the initial thread `cwd` and `executionContext`
- `thread/resume`: replaces the thread `cwd` and `executionContext` with the
  client's selected values
- `thread/fork`: replaces the forked thread `cwd` and `executionContext` with
  the client's selected values
- `turn/start`: updates the thread `cwd` and `executionContext` again for the
  client that submitted the turn

## Why send `executionContext` on every `turn/start`

This is redundant, but it matches current `cwd` behavior.

`turn/start` already carries a `cwd` override that updates the thread state for
that turn and subsequent turns. In a world with multiple connected clients,
whichever client submits the turn effectively wins for thread `cwd`.

For v1, `executionContext` should follow the same rule:

- whichever client sends `turn/start` also sends the `executionContext`
  associated with that `cwd`
- the server stores both

This is not the final architecture we probably want, but it is consistent with
the current thread-level `cwd` model and does not require introducing per-client
environment state yet.

## Client-side caching

Nix environment computation can be slow, so the client must cache by `cwd`.

The cache key should be the normalized absolute `cwd`.

The cache value should be one of:

- `Some(ExecutionContext)` when the client successfully derived one
- `None` when the client determined that no execution context applies for that
  `cwd`

That means the client should memoize both positive and negative lookups.

### Cache behavior

When the client needs an environment for a `cwd`:

1. Normalize the `cwd`.
2. Look in the cache.
3. If present, reuse the cached result.
4. If absent, evaluate once and store the result.

This cache should be used for:

- startup `cwd`
- resume/fork selected `cwd`
- later `turn/start` submissions

The client should not rerun Nix every time it needs to send the same
`executionContext`.

### Future invalidation

A future improvement should notice relevant `flake.nix` changes and invalidate
or refresh the cache entry for that `cwd`.

That is explicitly out of scope for v1, but the cache should be implemented in a
way that leaves room for invalidation later.

## Resume/fork behavior

For resume/fork, the TUI may ask the user whether to use the current `cwd` or
the saved session `cwd`.

Whichever `cwd` the user chooses, the client should derive or retrieve the
cached `executionContext` for that chosen `cwd` and send both together.

So the rule is:

- user chooses `cwd`
- client gets cached or computed `executionContext` for that `cwd`
- client sends both in `thread/resume` or `thread/fork`

## `--cwd` behavior

If the user launches the TUI with `--cwd`, that `cwd` becomes the initial
selected `cwd`.

The client should derive or retrieve the cached `executionContext` for that
`cwd` before sending `thread/start`.

This keeps `--cwd` consistent with every other path:

- selected `cwd`
- selected `executionContext`
- send both together

## Server behavior

The server should:

1. Accept `executionContext: { env: ... }` on the thread/turn requests listed
   above.
2. Store it on thread state.
3. Replace the stored value whenever a new one is supplied.
4. Use the stored env when spawning tool commands for that thread.

In practice, tool execution should build the environment roughly like this:

1. Start from the server's normal computed env.
2. Overlay `executionContext.env`.
3. Overlay any Codex-managed additions required at execution time.
4. Overlay per-call overrides as today.

If no `executionContext` is present, behavior remains unchanged.

## Validation and safety

Even though the server is only accepting structured data, it should still
validate it:

- environment names must be valid variable names
- values must be strings
- total entry count should be bounded
- total serialized size should be bounded

The server should not:

- execute client-provided shell code
- interpret `executionContext` as a script
- attempt to "derive" the environment by running a client-provided command

## Proposed API shape

Add an optional `executionContext` field to:

- `ThreadStartParams`
- `ThreadResumeParams`
- `ThreadForkParams`
- `TurnStartParams`

For v1:

```rust
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ExecutionContext {
    pub env: BTreeMap<String, String>,
}
```

Then add:

```rust
#[ts(optional = nullable)]
pub execution_context: Option<ExecutionContext>,
```

to each request above.

## Non-goals for v1

These are intentionally deferred:

- representing shell functions
- sending shell activation scripts
- tracking environment separately per client connection
- merging multiple clients' environments
- automatic support for `direnv`
- automatic support for `.env`
- attempting to preserve arbitrary "current shell" mutations

## Summary

The v1 design is:

- generic protocol: `executionContext: { env: { ... } }`
- client-side environment derivation
- Nix-based derivation first via `nix print-dev-env --json`
- thread-scoped storage on the server
- send `executionContext` anywhere connected TUI already sends `cwd`
- cache by normalized `cwd` so Nix evaluation is not repeated

This keeps the protocol simple, keeps the server dumb, and fixes the main
connected-TUI environment gap without taking on per-client server state yet.
