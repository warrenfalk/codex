# Keep Side Conversation as a Fork

Side conversations support exploration without disrupting their parent, while preserving useful
conversation history if the client or server crashes.

Every new side conversation is a durable fork from creation, with its parent lineage and all side
messages retained. A crash or disconnect never deletes it. Opening it in a fresh client treats it
as a normal fork, with ordinary commands and capabilities. Inherited parent history remains
reference context; reopening must not restart the parent's unfinished task unless a later user
message explicitly asks to continue it.

At creation, its saved name is `(Side) <parent name>`, using the parent's current name once.
An unnamed parent (or unavailable parent name) uses `(Side) <side thread UUID>` instead.
Later parent renames and automatic title generation do not replace this name. Recovering
the fork retains the prefix; users can rename it normally afterward. A naming failure must not
prevent access to the already-created fork.

The idle side-conversation close menu includes **Keep as a fork**, alongside Cancel, Summarize,
Summarize for me, and Leave. Cancel remains the default. Keep as a fork prompts for a new,
nonempty fork name, then retains the same thread and its history and returns to the previous
conversation. Canceling the name prompt leaves the side conversation open and unchanged.

Inside a side conversation, `/rename` opens the normal naming flow; `/rename <name>` also works.
Successfully naming the side conversation promotes it to a normal fork and continues in place.
Canceling the naming flow leaves it in side mode. `/fork` remains disabled in side conversations
and explains: "Use /rename to keep this side conversation as a fork."

Neither promotion path copies the conversation, starts a conversation turn, or modifies the parent.
Side-only command restrictions and temporary model instructions end when the conversation is
kept or opened as an ordinary fork.
Keeping appends a durable instruction explicitly ending side mode before restoring normal
capabilities. It preserves existing history in place, including the boundary and prior side turns,
without starting inference. If saving the name or this transition fails, the conversation stays
open in side mode and reports the failure. A name saved before a transition failure may remain,
but the conversation must not be promoted or navigated away from on that failure.

Leave returns to the parent and permanently deletes the side conversation. Both summary choices
save their result to the parent before deleting the side conversation. Summary generation or
parent-save failures retain the side conversation. Explicit close failures show an error and
preserve access to the side conversation. Ordinary thread navigation and the side-conversation
toggle retain an unkept side conversation; deletion requires an explicit close choice or lifecycle
cleanup.

Side mode remains intended for questions and lightweight exploration. Its temporary restrictions
apply to individual side turns, while the boundary separating reference-only parent history from
active side instructions remains meaningful after keeping or recovering the fork.
The boundary is explicitly supersedable by later instructions, and later user messages can adopt
tasks from inherited history. Compaction input is unchanged: keeping does not remove or rewrite
history to retire the boundary or side restrictions, preserving the existing inference prefix.

Validation must cover both close-menu widths and keyboard selection, persisted parent lineage,
side history surviving a fresh server/client attachment, both promotion paths keeping the same
thread with normal commands restored, close-menu naming and return to the parent, `/rename`
promotion in place with and without an inline name, canceling either naming flow without promotion,
`/fork` guidance, scoped side instructions ending on keep/recovery, explicit deletion without
changing the parent, and failure paths retaining the fork.
Also cover named and unnamed parents, names surviving recovery and later parent renames, and
ordinary user renaming after keeping the fork.
Keeping must append the model-visible transition after existing history, preserve it through
resume, and retain side mode if that transition cannot be saved.
