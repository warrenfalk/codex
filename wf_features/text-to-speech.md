# Text to speech for assistant answers

Codex can read its messages aloud through a local speech command. This supports
conversational work such as grilling sessions without requiring the user to watch
every answer or question on screen.

## Modes and controls

- `/speak` opens a selector with three choices: **Off**, **Final**, and **Progress
  and final**. Final speaks final answers, proposed plans, and interactive
  questions with their choices. Progress and final additionally speaks assistant
  progress messages. Messages without phase metadata count as final answers.
- `/speak off`, `/speak final`, and `/speak progress-and-final` select a mode directly.
  These controls work during running turns and side conversations.
- Changing from Off to an active mode while no agent turn is running immediately
  reads the most recently completed response, including a restored response or
  proposed plan. Turning speech off and on again can replay that response. Enabling
  speech mid-turn waits for subsequent completed messages; changing between active
  modes or selecting the current mode does not replay the previous response.
- `/speak stop` cancels active playback and discards pending speech without changing
  the selected mode. Later messages can still be spoken. While speech is playing
  or queued, Escape and Ctrl+C stop it and consume that keypress, preserving the
  draft, open views, and running turn. Once playback stops, those keys resume their
  usual navigation, editing, interruption, and exit behavior.
- An active speech mode is visible above the composer, including while a turn is
  running. Changing modes stops pending speech. The selection lasts for the TUI
  process, including switching threads, starting new sessions, and side
  conversations. Switching threads stops the previous thread's playback.
- Resuming, switching threads, and loading older history do not automatically
  narrate historical messages or questions. Explicitly enabling speech while idle
  can read the latest restored response. Repeated delivery of a recent completed
  message does not speak it twice. Only messages from the currently viewed
  conversation are spoken.

## Configuration and command contract

The initial mode and speech command are configurable in `config.toml`:

```toml
[tui.tts]
defaultMode = "off" # "off", "final", or "progress-and-final"
command = ["say"]
```

Both fields are optional and have the defaults shown above. Each new TUI instance
starts in `defaultMode`. Selecting a mode with `/speak` affects only the running TUI;
it does not rewrite this startup preference or change the mode of future instances.
Restart Codex after editing these settings in `config.toml`.

The command is an executable followed by optional arguments, for example
`command = ["/absolute/path/to/say", "--voice", "Charles"]` for a speech program
that supports those flags. Codex invokes it on the terminal's machine, including
when connected to a remote app server. Executable names resolve through PATH.
Arguments receive no shell expansion; shell operators and `~` are not expanded.

Each completed message is sent as UTF-8 on stdin, followed by EOF. The command must
wait for playback to finish and cancel its playback when terminated. Codex keeps
the terminal responsive and speaks messages in order without overlapping its own
commands. Command output does not overwrite the TUI. Exiting the TUI cancels its
speech command and discards pending speech.

Narration begins when a message completes, using its authoritative final text.
Markdown formatting and link destinations are omitted, inline code and link labels
remain readable, and fenced/indented code blocks and HTML blocks are skipped.
Internal reasoning, tool output, user messages, and notes to self are not spoken.

A missing/failing command or an overfull speech queue turns the mode off and shows
an error; chat continues normally. Speech retains at most 32 queued messages, each
at most 64 KiB of narration. Oversized messages produce an error instead of silent
truncation. The user can select a speech mode again to retry. Errors from cancelled
playback must not disable a newer selection.

## Validation expectations

Verify mode selection and the visible indicator with snapshots, including a narrow
terminal and a running turn. Verify idle activation reads only the latest response,
mid-turn activation waits for new messages, and Escape/Ctrl+C cancel playback before
other key handlers while retaining the speech mode. Verify live final/progress
filtering, both kinds of interactive questions and choices, legacy messages without
phases, history replay suppression, and duplicate delivery. Use a fake command to
verify literal argv, UTF-8 stdin, serial playback, cancellation, shutdown cleanup,
failure recovery, and bounded buffering without requiring audio hardware. Check
layered configuration loading and regenerate the config schema when settings change.
