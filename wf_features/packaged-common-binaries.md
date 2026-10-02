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

## Why it matters

The packaged CLI should feel predictable. When Codex can depend on a known
baseline toolbelt, shell-driven workflows work more reliably and the model does
not have to guess whether common utilities are installed.

Original implementation commit: `c2b4df2478` (`core: package common binaries`)
