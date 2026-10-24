# Device access inside the Linux sandbox

## Intent

Let agents use explicitly permitted host devices, such as GPU render nodes, while keeping
ordinary filesystem restrictions and command approval rules in effect.

## Required behavior

- On Linux, a filesystem `write` grant for `/dev` or a path beneath it permits device access
  within that grant. It can name one device or a device directory such as `/dev/dri`.
- Use the existing filesystem permission configuration; no new access mode is required:

  ```toml
  [permissions.trusted-workspace-conservative.filesystem]
  "/dev/dri" = "write"
  ```

- Resolve symlinks consistently with other writable grants. A granted symlink whose target
  is beneath `/dev` permits access to that target.
- A device grant keeps the command sandboxed. Unrelated filesystem paths, network access,
  and command approvals retain their existing restrictions.
- Denied paths and read-only overrides remain enforced beneath a granted device directory.
  A `read` grant alone does not authorize read/write use of a device.
- The default sandbox still exposes only its standard device set. Ordinary writable
  directories outside `/dev` do not implicitly enable device access.
- Grants do not elevate the agent's operating-system privileges. Device ownership, groups,
  access controls, and driver restrictions still apply.
- Device grants do not infer repository metadata protections for `.git`, `.agents`, or
  `.codex` beneath `/dev`. Explicit `read` and `deny` rules for those names still apply.
  This permits root-owned device directories and individual nodes without trying to create
  repository metadata placeholders there. Ordinary workspace grants keep those protections.
- Missing device paths follow the existing handling for missing writable grants, so a profile
  can be shared with an executor that lacks the corresponding hardware.
- This feature applies to the Linux sandbox, including Linux execution on a remote host.

## Validation expectations

- Open a granted device for reading and writing under the actual sandbox. Listing the path
  is insufficient to demonstrate usable device access.
- Cover individual devices, device directories, and symlink grants without requiring GPU
  hardware on the test host.
- Verify that denied devices, denied ordinary files, and read-only files remain protected,
  while allowed ordinary file writes still succeed.
