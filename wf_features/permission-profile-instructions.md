# Permission Profile Instructions

## Intent

Let a named permission profile define how the agent should work within its sandbox. Users can change escalation guidance without rebuilding Codex or maintaining a model catalog, and can inherit that guidance alongside filesystem and network permissions.

## Configuration

```toml
approval_policy = "trust-sandbox"
default_permissions = "my-workspace"

[permissions.my-workspace]
extends = ":workspace"
instructions_file = "policies/my-workspace.md"
```

The file contains literal UTF-8 Markdown. Relative paths resolve against the configuration file that declares them, including when another profile inherits the setting from a different configuration directory.

## Behavior

- A profile's `instructions_file` replaces any inherited instruction file. Omitting it inherits the nearest ancestor's file.
- If no profile in the inheritance chain supplies a file, existing built-in and model-catalog instruction selection continues to apply.
- An empty file explicitly supplies no behavioral permission instructions; it does not fall back to an ancestor or the defaults.
- Custom text replaces the behavioral permission and approval guidance, including trust-sandbox additions, file-edit guidance, and model-catalog overrides. Changing models must not replace a profile's custom guidance.
- Codex still supplies factual context about the effective sandbox mode, approval policy and reviewer, granular approval categories, network access, writable roots, denied reads, and approved command prefixes. These facts reflect the current execution environment rather than values copied into the instruction file.
- The setting changes model guidance only. Runtime sandbox enforcement, command approval rules, and permission grants keep their existing meaning.
- The active profile determines the instructions. Switching profiles or resuming with a persisted profile selection must not retain the configured default profile's guidance. An unnamed runtime permission override uses the default instruction behavior.
- Each custom block explicitly replaces earlier permission blocks until a later block replaces it. Profile changes append updated guidance without rewriting prior conversation history.
- Disabling `include_permissions_instructions` also disables the custom instruction block.

Instruction files are loaded when configuration is loaded, for all configured profiles, so later profile switches use the same loaded text. Changes on disk take effect on the next configuration load. Unreadable files, invalid UTF-8, and files larger than 4,000 bytes are configuration errors that identify the profile and path.

## Validation

Cover inheritance through multiple ancestors, a child replacement, an empty replacement, and relative paths originating in different configuration directories. Check file-read errors and the size boundary.

Inspect actual model requests to verify initial profile selection, model changes, profile switches, empty overrides, and resume. Verify that fallback works after selecting an unnamed runtime profile, and that custom instructions preserve factual permissions while replacing model-catalog and built-in behavioral guidance, including both trust-sandbox policies.
