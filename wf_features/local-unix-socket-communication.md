# Local Unix Socket Communication

## What it adds

This feature expands the Linux restricted-network sandbox so local Unix-domain
socket IPC keeps working even while real network sockets stay blocked.

## Final behavior

- `AF_UNIX` sockets remain allowed under restricted-network sandboxing.
- Path-based Unix socket workflows now work, including `bind`, `listen`,
  `connect`, `getsockname`, `getpeername`, and `getsockopt` after connection.
- Internet socket families still fail closed because non-Unix `socket(...)`
  calls are denied.
- The tests cover both socketpair-based IPC and path-based Unix socket server
  and client flows.

## Why it matters

Some local tooling uses Unix sockets for process coordination, helper daemons,
or bridge processes. This change lets that local IPC work without weakening the
sandbox's ban on real network traffic.

Original implementation commit: `d51d327e17` (`sandbox: permit local Unix socket communication`)
