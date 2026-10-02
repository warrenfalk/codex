# Desktop Link Handlers

## Intent

Clicking an editor link in the TUI should open the file in the application registered
for that URL scheme. It must not first open a browser tab and require a second
confirmation to launch the editor.

## Behavior to preserve

- Link clicks handled by Codex's interactive transcript use the desktop's
  registered URL handler on the machine running the TUI, including when connected
  to a remote app-server.
- Editor links such as `vscode:` and `cursor:` go directly to their registered
  application. HTTP and HTTPS links use their registered desktop application,
  including a browser picker when configured.
- The existing `file_opener` setting still chooses the editor URL scheme;
  `file_opener = "none"` still disables generated editor links. No additional
  Codex setting or `BROWSER` override is required.
- The full URL is passed unchanged, preserving encoded paths, line and column
  numbers, queries, and fragments. URL text is never interpreted as shell code.
- Successful handoffs add no confirmation message to the transcript.
- A missing launcher or unsuccessful handoff produces a visible error naming the
  URL. Codex does not retry the URL through a web browser.
- The TUI remains responsive while the desktop handles the request. Handoff
  success means the desktop accepted the request, not that the editor finished
  loading the file.
- Linux, macOS, and Windows use their native desktop URL associations. Built-in
  browser actions such as account management retain their existing behavior.
- Hyperlinks handled by the terminal itself in native scrollback mode continue
  to follow the terminal's own link-opening preferences.

## Validation expectations

- Verify editor URLs reach the desktop launcher unchanged even when `BROWSER`
  points at a different program, with no browser detour or success chatter.
- Verify web URLs, encoded spaces, line/column suffixes, and shell metacharacters
  survive the handoff intact.
- Cover missing launchers and nonzero exit status, including a snapshot of the
  visible error.
- On a desktop, click an editor link and confirm it opens the expected file and
  location directly. Verify an HTTPS link still uses the registered web handler.
