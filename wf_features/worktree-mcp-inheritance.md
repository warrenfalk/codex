# MCP defaults from the main checkout

Linked Git worktrees automatically share the project's MCP integrations. A fresh
worktree can use an MCP server defined in the main checkout's `.codex/config.toml`
without copying that file or moving a project-specific integration into user config.

## Behavior

- Inheritance is automatic for validated linked Git worktrees whose applicable
  project trust permits loading project config. An explicit untrusted worktree
  decision prevents inheritance. A forged or unregistered worktree pointer must
  never grant access to another checkout's configuration.
- Only `mcp_servers` is inherited. Other project settings and discovery of
  skills, agents, hooks, and exec-policy files retain their existing behavior.
- The main checkout's config files corresponding to directories between the
  worktree's discovered project root and working directory supply defaults.
  More specific main-checkout defaults override less specific ones. Every
  worktree-local project layer takes precedence over those defaults.
- Inheritance works even when the worktree has no `.codex` directory or config
  file. An unrelated local setting or MCP definition does not suppress it.
- MCP definitions merge by server name and field. Worktrees can change a URL,
  timeout, tool selection, or other setting without repeating unchanged fields.
  Arrays follow normal config replacement behavior.
- A worktree can disable an inherited server with just
  `[mcp_servers.<name>]` and `enabled = false`.
- When an override changes an inherited server between HTTP and stdio, settings
  belonging to the displaced transport must not leak into the effective server.
  Explicitly conflicting fields in the override remain configuration errors.
- Existing user, session, and managed configuration precedence still applies.
  Authentication uses the normal MCP credential mechanisms; inheritance does
  not install a connector plugin or establish a new authenticated connection.
- Source metadata identifies the actual main-checkout config file for inherited
  fields and the worktree file for overridden fields. Explicit MCP working
  directories and command arguments retain normal MCP interpretation; the
  session's working directory remains the worktree.
- Config is read during normal configuration loading, without copying or
  modifying files. Main-checkout edits become visible on subsequent loads;
  this feature does not promise immediate updates to an already running session.
- A missing main config supplies no defaults. Read or TOML parse errors in an
  applicable trusted source identify that main-checkout file. Strict validation
  checks inherited MCP settings without validating unrelated main-only settings.
- Normal and executor-local config reads must agree, using the executor's
  filesystem for Git metadata and config access.

## Validation

Exercise config loading for fresh and populated worktrees, field overrides,
explicit disabling, additional local servers, nested main defaults and local
precedence, both transport changes, source attribution, reload after a main edit,
ordinary checkouts, missing config, explicit distrust, invalid worktree
registration, malformed config, and strict validation. Verify that inherited
MCP projections do not add roots for discovery of other project artifacts and
that normal and executor-local reads produce the same effective MCP definitions.
