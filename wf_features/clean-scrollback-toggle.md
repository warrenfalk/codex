# Clean Scrollback Toggle

The TUI provides a session-local clean scrollback mode for temporarily hiding committed history
items that are useful for auditing but noisy while reading the conversation. The default shortcut is
`Alt+N`, exposed through the normal keymap action `global.toggle_clean_scrollback`.

When clean scrollback is enabled, the visible terminal transcript and transcript overlay keep
conversation-focused history:

- session headers and session info
- user messages
- assistant messages
- reasoning summaries
- proposed plans and plan updates
- review-mode markers
- context-compaction markers

Committed tool, command, file-change, status, warning, approval, MCP, web, image, hook, and
subagent status history is hidden while the mode is enabled. New noisy history produced while the
mode is enabled is still stored in memory, but it is not inserted into the visible scrollback until
the mode is turned off.

Toggling the mode off restores all transcript cells still available in the current TUI process. The
mode starts off for every launch, resume, and clear. `/clear` resets it along with other transcript
UI state.

Clean scrollback is a temporary display filter only. It does not change rollout persistence,
app-server resume behavior, stored thread history, or historical reconstruction. If a session is
resumed later, the TUI receives whatever reconstructed history the normal app-server resume path
provides; clean scrollback does not persist a separate full transcript.

Terminal resize reflow and the transcript overlay must honor the current clean scrollback state so
hidden cells do not reappear until the user toggles the mode off.
