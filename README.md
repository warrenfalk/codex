# Codex CLI for Local Agent Work

This fork keeps the OpenAI Codex CLI foundation and turns it into a stronger
local agent workspace: terminal sessions can share a backend, browser and phone
clients can control those sessions, transcripts link back into your editor, and
the packaged runtime comes with the tools an agent needs to get real work done.

The original upstream README is preserved at [README_CODEX.md](README_CODEX.md).

## Quickstart From This Checkout

Build and run the fork locally:

```sh
nix build
./result/bin/codex
```

For development inside the repo:

```sh
nix develop
just codex "your prompt"
```

The Rust CLI and TUI live under `codex-rs/`. Fork-local feature notes live under
`wf_features/`.

## The Headline Feature: Remote Control

The local shared backend makes Codex controllable from more than one surface at
the same time. A TUI can stay open on the workstation while a separate
mobile-first web app connects to the same local app-server and controls those
sessions from a browser or phone.

The remote-control website lives on the `remote-control-web` branch in the
standalone `web/` package. It connects to `codex app-server`, shows local
threads, follows live session updates, sends new prompts, interrupts running
turns, handles approval requests, and can be installed as a Home Screen app from
its HTTPS origin.

That is the main reason the shared backend exists: Codex stops being a single
terminal process and becomes a local agent backend with multiple useful clients.

See [local shared app server](wf_features/local-shared-app-server.md),
[connected auto-reconnect](wf_features/connected-auto-reconnect.md), and the
[`remote-control-web` web app](https://github.com/warrenfalk/codex/tree/remote-control-web/web).

## Feature Highlights

### Shared backend for local sessions

Run TUI sessions against a shared local app-server without giving up local
working-directory behavior. The resume picker, latest-session lookup, cwd
prompts, and normal local workflow still feel like local Codex, while the
session is available to other clients.

The TUI also shows when it is connected to a shared backend and automatically
recovers from backend restarts when it can.

See [connected footer badge](wf_features/connected-footer-badge.md).

### Better session identity

Saved sessions are easier to recognize because Codex generates short thread
titles automatically. Terminal/window titles also track the active Codex view,
so multiple Codex windows from the same repo are easier to tell apart.

See [automatic thread title](wf_features/automatic-thread-title.md) and
[active view terminal titles](wf_features/active-view-terminal-titles.md).

### Links in the terminal transcript

When the assistant references a file, Codex turns that reference into a
clickable terminal link. When the assistant includes a normal markdown link,
that link is clickable too. Review output becomes something you can navigate
directly instead of text you have to copy by hand.

See [clickable assistant file references](wf_features/assistant-file-references-clickable.md).

### File mentions for real worktrees

`@file` completion can surface ignored files when you explicitly type into the
local path that contains them. This keeps broad search clean while still letting
you mention generated files, local-only config, or ignored scratch artifacts
when you actually ask for them.

See [file mentions include path-local ignored files](wf_features/file-mentions-include-ignored-files.md).

### Quota pacing instead of raw quota numbers

The status card, footer, and configurable status line show whether usage is
ahead of or behind pace for the current limit window. Labels such as
`weekly +40%` make the useful question visible: not just how much quota remains,
but whether the current pace is sustainable.

See [usage limit pace indicator](wf_features/usage-limit-pace-indicator.md).

### Faster composer workflow

`Ctrl+I` copies the current unsent prompt from the composer. This is useful when
a prompt needs to move into another tool, another Codex session, or a bug report
without sending it first.

See [copy unsent prompt text](wf_features/copy-unsent-prompt-text.md).

### Practical local sandboxing and packaging

Restricted-network Linux sandboxing still allows local Unix-domain socket IPC,
so helper daemons and bridge processes can coordinate locally without opening
internet sockets.

The Nix-packaged CLI also includes a curated baseline toolbelt on `PATH`,
including common development tools and PDF utilities, so shell-driven agent work
is less dependent on whatever happens to be installed on the host.

See [local Unix socket communication](wf_features/local-unix-socket-communication.md)
and [packaged common binaries](wf_features/packaged-common-binaries.md).

### Clear fork identity

Custom builds identify themselves in `codex --version` and in the TUI session
header, making screenshots, logs, and bug reports much easier to interpret.

See [custom build version labels](wf_features/custom-build-version-labels.md).

## Bugs Fixed And Quality Improvements

- Connected TUI sessions no longer imply context usage before that data is
  actually known.
- Stale MCP startup banners are cleared after resume.
- Session recording retries after transient persistence failures instead of
  treating one write failure as permanent loss.
- Workspace-write overrides for `apply_patch` and approval flows are fixed.
- Linux/Nix build prerequisites are restored for this fork.
- Linux sandbox debug builds avoid fortify warning noise.

## Feature Notes

- [Active view terminal titles](wf_features/active-view-terminal-titles.md)
- [Automatic thread title](wf_features/automatic-thread-title.md)
- [Clickable assistant file references](wf_features/assistant-file-references-clickable.md)
- [Connected auto-reconnect](wf_features/connected-auto-reconnect.md)
- [Connected footer badge](wf_features/connected-footer-badge.md)
- [Copy unsent prompt text](wf_features/copy-unsent-prompt-text.md)
- [Custom build version labels](wf_features/custom-build-version-labels.md)
- [File mentions include path-local ignored files](wf_features/file-mentions-include-ignored-files.md)
- [Local shared app server](wf_features/local-shared-app-server.md)
- [Local Unix socket communication](wf_features/local-unix-socket-communication.md)
- [Packaged common binaries](wf_features/packaged-common-binaries.md)
- [Session persistence retries](wf_features/session-persistence-retries.md)
- [Usage limit pace indicator](wf_features/usage-limit-pace-indicator.md)

## Upstream Docs

For the upstream project overview, install instructions, and general Codex links,
see [README_CODEX.md](README_CODEX.md). This repository remains licensed under
the [Apache-2.0 License](LICENSE).
