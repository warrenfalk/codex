# Project mentions

The interactive terminal can attach another project directory while composing a prompt, without
restarting the conversation or changing the working directory.

## Choosing a project

- The unified `@` mention picker includes a Projects category. Project choices are distinct from
  ordinary file and directory references; those references never change access.
- Recent session working directories on the connected server supply suggestions, deduplicated by
  directory and ordered by recent activity. Discovery is bounded; it is not a complete project registry.
  Separate worktrees remain separate choices. Already attached roots are omitted.
- Each project row includes its full path to distinguish projects with the same name.
- “Choose another directory…” starts directory path completion. Completion lists directories on the
  connected server, including paths outside the current project. It does not recursively scan the disk.
- The Projects picker explains that submitting attaches the directory for the session under the
  current permission profile. Selecting a row inserts a bound mention; removing it before submission
  cancels the attachment. Plain text resembling a mention does not attach anything.

## Session behavior

- Submitting a project mention adds its directory to the existing runtime workspace roots for that
  turn and later turns. Existing roots, working directory, profile selection, network restrictions,
  and denied paths are preserved. Access remains subject to the selected profile and managed rules.
- An attachment is not a request to load the other project's configuration or instructions.
- Prompts that need a new attachment wait for an active turn to finish; steering cannot change the
  active turn's roots. A failed attachment must not silently submit work without the requested roots.
- Missing directories and files offered by stale history are rejected before the turn starts.
- Accepted roots stay with the owning thread when navigating between conversations, resuming, or
  forking, using existing server persistence. Mention bindings survive draft/history restoration.
- This feature uses existing app-server APIs and requires no server or protocol additions.

## Validation

Cover bound selection versus plain text, removal before submission, duplicate directory names,
spaces and non-ASCII paths, directory completion, discovery limits, stale/missing paths, active-turn
queueing, preservation of existing roots and permission restrictions, and later-turn persistence.
Snapshot the project picker and attachment feedback. Exercise the real app-server boundary for
root updates and persistence.
