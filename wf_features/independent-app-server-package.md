# Independent app-server package

The Nix app-server package can be updated independently of the interactive Codex
command. Editing the TUI must not replace the executable used by a supervised
server or restart that server during a workstation update.

## Preserved behavior

- `nix build .#codex-app-server` provides `codex-app-server`, accepting the existing
  server arguments, including `--listen unix://`, stdio, and WebSocket endpoints.
- The package includes the matching local code-mode host, the Linux bubblewrap
  helper when applicable, and the same baseline shell toolbelt as the packaged
  `codex` command. Local execution, sandboxing, and
  code-mode tools continue to work without a separately installed combined CLI.
- The default package and `codex` command retain their existing behavior,
  including the TUI, resume, fork, remote connections, and embedded-server fallback.
- TUI and combined-CLI source edits leave the server package's Nix store path
  unchanged. Server source, shared dependencies, the shared Cargo lockfile, and
  relevant packaging or toolchain changes may rebuild it.
- Development builds must not derive the server version from the whole
  repository revision, which would reintroduce replacement after frontend edits.
- Linux and macOS supervisors may point directly at the separate server package.
  Switching an existing installation to this package replaces the service once;
  subsequent frontend-only updates leave it running.
- Separately updated clients and servers must still speak compatible app-server
  protocols. This packaging split does not promise arbitrary version mixing.

## Validation

Build the default and standalone-server packages. Check that a TUI source edit
changes the default derivation while leaving the standalone-server derivation
unchanged, and that a server or shared backend source edit changes the latter.
Run `nix develop -c python3 scripts/check_app_server_package.py` for these
derivation checks, including helper changes and development-version handling.
Exercise the packaged server's public JSON-RPC initialization and a local
command through an isolated test instance, including its bundled toolbelt.
Verify that the supervised command uses the standalone package on Linux and
macOS. Workstation activation remains a separate user action.
