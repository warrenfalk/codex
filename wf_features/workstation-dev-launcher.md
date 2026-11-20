# Workstation development launcher

Run `dev codex [arguments...]` to build and launch the Codex CLI from this
checkout using the workstation's `dev` command. It requests a debug build on
each invocation and reuses incremental build output. A failed build exits
without launching Codex.

Codex starts in the directory where `dev codex` was invoked, inherits the
caller's environment, and receives each argument as passed. This lets you try
local Codex changes while working in another project.

Verify with `dev codex --version` from outside the Codex checkout.
