# TUI History Cell Spec

Derived from repository commit `f0c773b888d2a8e6ceac514f8741914d4bfb1b7e`.

This is an experimental Haskell specification for reducing decoded app-server and app/client inputs
into the event-sourced transcript state needed to render Codex TUI history cells.

The spec deliberately models semantic display cells rather than Rust `HistoryCell` implementation
families. Each Haskell cell definition includes comments naming the Rust render family and the
event provenance that motivated the definition.

## Scope

Included:

- Event-sourced render inputs carried by decoded app-server notifications, server-initiated
  requests, thread bootstrap/read snapshots, and app/client interaction events needed to resolve a
  pending transcript cell.
- Active/updatable cells, such as streaming assistant text, plan streams, command execution, MCP
  calls, web search, and hook runs.
- Pending non-transcript interactions, such as request-user-input prompts and approval requests,
  when later event-sourced input can use that state to produce or remove visible transcript state.
- Total reduction: orphan completions and deltas are tolerated and recorded as diagnostics instead
  of making reduction partial.

Excluded:

- Renderer logic: wrapping, truncation, styling, color choice, animation frames, elapsed wall-clock
  timers, and viewport-dependent layout.
- Local-only TUI command/decorative cells such as update-available, session help, status, usage,
  process summaries, feedback success, and tooltip cards.
- JSON-RPC/WebSocket framing. The reducer consumes decoded domain inputs, not raw transport
  envelopes.

## Files

- [src/TuiHistoryCellSpec.hs](src/TuiHistoryCellSpec.hs) contains the current executable-ish data
  model and reducer skeleton.
- The source provenance reference is [../tui_history_cell_provenance/README.md](../tui_history_cell_provenance/README.md).
