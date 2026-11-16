# Custom Build Version Labels

## What it adds

This feature makes this fork identify itself clearly in user-visible version
surfaces.

## Final behavior

- `codex --version` reports the package version with a `warrenfalk custom`
  suffix.
- The same custom-build label appears in the TUI session header instead of only
  the upstream version number.
- Test builds use a stable placeholder version so snapshots do not churn every
  time the package version changes.

## Why it matters

This makes it obvious when a user is running the fork rather than upstream
Codex. That helps when debugging behavior, comparing screenshots, or discussing
which build produced a given result.

Original implementation commit: `76e93f638f` (`cli: label custom builds in version output`)
