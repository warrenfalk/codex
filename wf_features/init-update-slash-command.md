# Init Update Slash Command

## Intent

`/init-update` gives users a direct way to create or refresh the current
repository's `AGENTS.md` without relying on the older `/init` behavior that
skips an existing file.

## Final behavior

- `/init-update` is a bare slash command. It does not accept inline arguments.
- Running it submits a purpose-built prompt as the next user turn.
- The prompt targets only `./AGENTS.md` in the current working directory.
- If `./AGENTS.md` exists, the agent must read it from disk, inspect the current
  repository, and update the file so it reflects current structure, commands,
  docs, and conventions.
- If `./AGENTS.md` does not exist, the agent must create a concise repository
  guide titled `Repository Guidelines`, following the same practical style as
  `/init`.
- Existing loaded instruction-source paths are not the authority for update
  behavior. The file on disk and current repository evidence are.
- While a task is running, `/init-update` is unavailable in the same way as
  `/init`.
- Queued `/init-update` input drains like `/init`: dispatching the command starts
  a user turn and stops further queue draining until that turn has begun.

## Validation expectations

- The slash command parser recognizes `/init-update` as a command, not literal
  prompt text.
- Dispatch submits exactly one user message containing the update prompt.
- Local slash-command recall records `/init-update`.
- The command is hidden from side-conversation-only command availability unless
  the normal side-conversation command rules change.
- Popup and command-list snapshots include `/init-update` with an accurate
  description.
