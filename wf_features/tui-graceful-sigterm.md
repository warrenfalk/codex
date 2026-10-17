# TUI Graceful SIGTERM

## Intent

External supervisors and terminal wrappers should have a signal-based way to ask
an interactive Codex TUI session to shut down without corrupting terminal input
mode or losing the normal resume hint.

## Final Behavior

On Unix platforms, the interactive TUI installs a SIGTERM listener after the app
event channel is available. The first SIGTERM requests the same shutdown-first
exit path used by explicit user quits such as `/quit`, `/exit`, and the
double-press Ctrl+C/Ctrl+D shortcut.

The shutdown-first path must:

- ask the active thread to shut down before the UI loop exits,
- use the existing shutdown timeout if the app-server or thread is wedged,
- clear the TUI surface,
- restore terminal modes, including raw mode, bracketed paste, focus reporting,
  alternate-scroll, cursor visibility, and keyboard enhancement reporting, and
- return normal `AppExitInfo` so the CLI prints token usage and the
  `To continue this session, run codex resume ...` hint when a persisted rollout
  is available.

SIGTERM must not behave like an in-TUI Ctrl+C keypress. It is an external process
request to quit, so it bypasses prompt-clearing, interrupt-only, popup, and
double-press confirmation behavior.

If a second SIGTERM arrives before the graceful exit finishes, Codex restores the
terminal best-effort and exits immediately with the conventional Unix
signal-exit status for SIGTERM. This force path is intentionally allowed to skip
thread cleanup and the resume hint; it exists so supervisors still have an
escalation path if the graceful shutdown gets stuck.

SIGKILL cannot be handled. Non-Unix platforms keep their existing behavior unless
they gain an explicit platform-specific graceful termination signal later.

## Validation Expectations

Validation should cover that the first SIGTERM listener event sends
`AppEvent::Exit(ExitMode::ShutdownFirst)`, that a second signal takes the force
path instead of queuing another graceful exit, and that the normal TUI exit path
continues to produce the resume hint for resumable persisted threads.
