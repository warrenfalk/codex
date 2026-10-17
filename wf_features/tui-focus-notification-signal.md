# TUI Focus Notification Signal

## Intent

External tools that already know the process ID of a particular interactive
Codex TUI should have a harmless way to ask that session to present a desktop
notification which the user can click to return to its terminal window.

## Final Behavior

On Unix platforms, sending `SIGURG` to an interactive Codex TUI requests a
desktop notification with this message:

> Click to focus this Codex session

The notification uses the same behavior as other TUI notifications. In
particular:

- its notification type is `focus-requested`,
- disabling TUI notifications suppresses it,
- a custom notification list must include `focus-requested` to allow it,
- the configured notification condition still applies, so the default
  `unfocused` condition suppresses the notification when the terminal is
  already focused,
- normal backend selection and failure handling apply, and
- normal coalescing applies. The focus request has low priority, so it does not
  replace a pending approval or interactive prompt notification.

Where the terminal supports focusing the originating window after a desktop
notification is clicked, clicking this notification returns to the targeted
Codex session. The signal itself does not focus the window automatically.

`SIGURG` is ignored by default when no handler is installed. Sending it before
the interactive TUI installs its listener, or to another Codex process that
does not handle it, must therefore remain harmless. Non-Unix platforms retain
their existing behavior.

The caller is responsible for discovering and targeting the exact TUI process
ID. This feature does not provide process discovery, PID files, session lookup,
or broadcast targeting. A typical invocation is:

```sh
kill -URG <tui-pid>
```

## Validation Expectations

Validation should cover repeated signal delivery, the exact user-visible
message, `focus-requested` custom filtering, disabled-notification suppression,
and preservation of pending higher-priority interactive notifications.
