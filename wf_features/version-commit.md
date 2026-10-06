# Installed source commit

`codex --version-commit` identifies the source revision of the installed CLI so
its fork commits can be inspected in a separate repository checkout, even when
multiple builds share the same upstream version number.

- Print only the build's source commit followed by a newline, then exit
  successfully without loading configuration or starting an interactive session.
- Clean Git-backed Nix builds print the full source SHA. Dirty Git-backed builds
  print the source SHA with a `-dirty` suffix when that information is available.
- Builds without source revision metadata print `unknown`. Cargo builds can
  supply the revision through `STABLE_GIT_COMMIT` at compilation time.
- The result identifies this executable's build, regardless of the current
  directory, runtime environment, or connected server. No runtime Git lookup is
  needed.
- Ordinary `--version` output retains its existing behavior. Commit lists and
  feature summaries are outside this command's scope.
- Frontend revision changes must preserve the independently packaged server's
  existing build isolation.

Validation should exercise the installed command outside a repository with an
invalid user configuration, verify that a runtime environment override cannot
change the output, and check Nix frontend/server isolation across revisions.
