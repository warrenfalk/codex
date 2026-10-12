# Keep Side Conversation as a Fork

Side conversations support exploration without disrupting their parent, while preserving useful
conversation history if the client or server crashes.

Every new side conversation is a durable fork from creation, with its parent lineage and all side
messages retained. A crash or disconnect never deletes it. Opening it in a fresh client treats it
as a normal fork, with ordinary commands and capabilities. Inherited parent history remains
reference context; reopening must not restart the parent's unfinished task.

At creation, its saved name is `(Side) <parent name>`, using the parent's current name once.
An unnamed parent (or unavailable parent name) uses `(Side) <side thread UUID>` instead.
Later parent renames and automatic title generation do not replace this name. Keeping or recovering
the fork retains the prefix; users can rename it normally afterward. A naming failure must not
prevent access to the already-created fork.

The idle side-conversation close menu includes **Keep as a fork**, alongside Cancel, Summarize,
Summarize for me, and Leave. Cancel remains the default. Keep as a fork retains the same thread
and its history and lets the user continue in it as a normal fork. It does not copy the conversation,
start inference, or modify the parent. Side-only command restrictions and temporary model
instructions end when the conversation is kept or opened as an ordinary fork.

Leave returns to the parent and permanently deletes the side conversation. Both summary choices
save their result to the parent before deleting the side conversation. Summary generation or
parent-save failures retain the side conversation. Explicit close failures show an error and
preserve access to the side conversation. Existing navigation away from an unkept side still
discards it; a failed background deletion leaves its durable fork available for recovery.

Side mode remains intended for questions and lightweight exploration. Its temporary restrictions
apply to individual side turns, while the boundary separating reference-only parent history from
active side instructions remains meaningful after keeping or recovering the fork.

Validation must cover both close-menu widths and keyboard selection, persisted parent lineage,
side history surviving a fresh server/client attachment, keeping the same thread with normal
commands restored, scoped side instructions ending on keep/recovery, explicit deletion without
changing the parent, and failure paths retaining the fork.
Also cover named and unnamed parents, names surviving recovery and later parent renames, and
ordinary user renaming after keeping the fork.
