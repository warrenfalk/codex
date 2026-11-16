# Packaged Common Binaries

## What it adds

This feature gives the packaged Nix build of Codex a small, predictable baseline
toolbelt on `PATH` so the agent can rely on a curated set of common command-line
tools.

## Final behavior

- The packaged `codex` binary should include a wrapped `PATH` with a curated
  manifest-driven toolbelt.
- That baseline should cover common general-purpose tools such as `rg`, `git`,
  `python`, `python3`, `sqlite3`, `file`, `strings`, and `xxd`.
- It should also include a focused set of PDF utilities such as `pdftotext`,
  `pdfinfo`, `pdfimages`, `pdftoppm`, `pdftocairo`, `pdftohtml`, `pdfgrep`,
  `mutool`, `qpdf`, and `gs`.
- The packaged experience should expose this as a safe assumption in model and
  prompt guidance: assume the curated toolbelt is present, but fall back
  gracefully if a command is still unavailable.
- The bundled tool list and prompt guidance should stay synchronized from a
  single manifest so packaging and agent instructions do not drift apart.
- The default Nix package provides the combined `codex` command, standalone
  app-server and exec-server commands, code-mode and voice hosts, and
  `apply_patch`. Linux packages also provide the Linux sandbox and bubblewrap
  helpers. The package's `codex-dev` launcher remains available.
- The combined CLI retains its TUI, noninteractive execution, resume/fork,
  server, and proxy subcommands. The Nix package does not provide separate
  standalone TUI/exec launchers, development fixtures, samples, schema writers,
  platform probes, or standalone utility/proxy commands. Those remain buildable
  from the development checkout when needed.

## Validation

Build the default Nix package and inspect its installed command inventory.
Exercise CLI/subcommand help, public app-server initialization, sandboxed command
execution, the curated toolbelt, and packaged code-mode execution from an isolated
home without development binaries on PATH. Keep the voice host present and check
its native library closure without claiming audio-device validation. Confirm the
independent app-server package retains its separate output and source isolation.

## Why it matters

The packaged CLI should feel predictable. When Codex can depend on a known
baseline toolbelt, shell-driven workflows work more reliably and the model does
not have to guess whether common utilities are installed.

Original implementation commit: `c2b4df2478` (`core: package common binaries`)
