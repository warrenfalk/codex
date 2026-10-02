# Linux PID namespace selection

Trusted workspace commands can inspect processes started outside their own tool
invocation without requiring an unsandboxed command approval.

## Configuration and inheritance

Permission profiles accept `pid_namespace = "isolated"` or `pid_namespace = "host"`:

```toml
[permissions.trusted-workspace]
extends = ":workspace"
pid_namespace = "host"

[permissions.trusted-workspace-conservative]
extends = "trusted-workspace"
```

An omitted value inherits through `extends`. If no ancestor specifies the setting,
the default is `isolated`. A child may explicitly restore `isolated`. Unknown
values are rejected. Existing profiles and previously saved sessions without the
setting retain their previous behavior.

## Observable behavior

- On Linux, `isolated` gives each bubblewrap invocation its own PID namespace and
  process list. Commands see processes created inside that invocation.
- `host` shares the executor's existing PID namespace. Process tools can see
  processes outside the invocation, and ordinary Linux permissions determine
  whether those processes can be signaled. This is not a read-only listing grant.
- `host` does not escape an enclosing container or PID namespace and grants no
  elevated OS privileges. Protected process details can remain inaccessible.
- Selecting `host` retains filesystem, network, user-namespace, and IPC sandbox
  restrictions. In particular, host process root and file-descriptor paths must
  not provide an alternate route to files denied by the filesystem policy.
- The setting survives profile selection, session persistence, workspace-root
  resolution, executor transport, and additional filesystem/network grants.
- Intersecting permissions retains PID isolation if either input requires it.
- This setting applies to Linux bubblewrap only. It does not enable sandboxing
  for commands that otherwise run without it, alter the legacy Landlock backend,
  or change macOS/Windows behavior. Existing command approval rules still apply.

## Validation expectations

Cover defaulting, inherited `host`, explicit child `isolated`, invalid values, and
loading saved profiles with and without the field. Verify that permission
transformations and executor transport retain the selected namespace.

Under the real Linux sandbox, compare parent-process visibility in both modes,
including normal process addressing in `host` mode. Verify writable paths remain
writable, read-only paths reject writes, and denied paths reject reads. Check
host `/proc/<pid>/root` and `/proc/<pid>/fd` access against a controlled parent
process, with both enabled and restricted networking.
