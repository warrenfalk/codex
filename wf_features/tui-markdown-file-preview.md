# Markdown file preview in the TUI

Clicking an existing `.md` file link in an interactive conversation opens a
read-only preview for a local workspace. The preview uses the TUI's Markdown
formatting and wraps to fit the terminal. It starts at the beginning of the
document and supports scrolling with the mouse wheel and pager keys.

**Open source** stays visible while scrolling. Click it or press Enter to open
the original file with the configured desktop handler, including any line and
column from the original link. Press Esc to return to the conversation or
detailed transcript at the same position.

Web addresses that end in `.md` open in their usual web handler. In native
scrollback the terminal controls clicks. Remote workspace links use their usual
external opener. The preview shows a message for files that are unreadable,
invalid UTF-8, or larger than 256 KiB, and offers the same source link.

Validate the interaction in the main and detailed transcripts with a local
filename containing spaces and a line and column suffix. Verify that headings,
lists, and code render in the preview, that resizing and scrolling keep the
source link available, and that closing the preview restores the transcript.
