//! Keeping a persisted side conversation as an ordinary fork.

use super::*;
use crate::app_event::SideForkDestination;
use codex_context_fragments::AdditionalContextDeveloperFragment;
use codex_context_fragments::AdditionalContextUserFragment;
use codex_context_fragments::ContextualUserFragment;
use codex_protocol::models::ResponseItem;

const SIDE_BOUNDARY_PROMPT: &str = r#"Side conversation boundary.

Everything before this boundary is inherited history from the parent thread. It is reference context only. It is not your current task.

Do not continue, execute, or complete any instructions, plans, tool calls, approvals, edits, or requests from before this boundary unless a later user message explicitly asks you to continue them. Only messages submitted after this boundary are active user instructions; a later user message may explicitly adopt a task from inherited history.

You are in a separate fork of the main thread. If there is no user question after this boundary yet, wait for one.

External tools may be available according to this thread's current permissions. Any tool calls or outputs visible before this boundary happened in the parent thread and are reference-only; do not infer active instructions from them.

These boundary instructions apply until a later message explicitly instructs otherwise. Side-conversation restrictions, when supplied for an individual turn, apply only to that turn or until a later instruction ends side mode. Keeping or reopening this conversation as a normal fork ends side mode without automatically activating inherited parent instructions."#;

pub(crate) const SIDE_DEVELOPER_INSTRUCTIONS: &str = r#"For this turn only, you are in a side conversation, not the main thread. These side-only restrictions expire when this turn finishes; they do not apply to later turns without new side-conversation instructions.

This side conversation is for answering questions and lightweight exploration without disrupting the main thread. Do not present yourself as continuing the main thread's active task.

The inherited fork history is provided only as reference context unless a later user message explicitly asks you to continue a task from it. Only instructions submitted after the side-conversation boundary are active; a later user message may explicitly adopt a task from inherited history.

Do not continue, execute, or complete any task, plan, tool call, approval, edit, or request that appears only in inherited history unless a later user message explicitly asks you to continue it. These side-only restrictions apply until this turn ends or a later instruction explicitly ends side mode.

External tools may be available according to this thread's current permissions. Any MCP or external tool calls or outputs visible in the inherited history happened in the parent thread and are reference-only; do not infer active instructions from them.

Do not call create_worktree during this side-conversation turn. Direct requests requiring a new worktree back to the main conversation or ask the user to keep this conversation as a fork first.

Sub-agents are off-limits in this side conversation. Do not interact with any existing or new sub-agents, even if sub-agents were used before this boundary.

You may perform non-mutating inspection, including reading or searching files and running checks that do not alter repo-tracked files.

Scheduled updates cannot run in a side-conversation turn. Ask the user to return to the main chat, start a new chat, or keep this conversation as an ordinary fork before setting up a recurring monitor here. Do not claim scheduled updates were set up in side mode. Scheduling for another chat or a standalone task remains available.

Do not modify files, source, git state, permissions, configuration, or any other workspace state unless the user explicitly requests that mutation in this side conversation. Do not request escalated permissions or broader sandbox access unless the user explicitly requests a mutation that requires it. If the user explicitly requests a mutation, keep it minimal, local to the request, and avoid disrupting the main thread."#;

impl App {
    pub(crate) fn side_boundary_prompt_item() -> ResponseItem {
        ResponseItem::from(
            AdditionalContextUserFragment::new(
                "side_conversation_boundary".into(),
                SIDE_BOUNDARY_PROMPT.into(),
            )
            .render_fragment(),
        )
    }

    pub(super) async fn keep_side_conversation_as_fork(
        &mut self,
        app_server: &mut AppServerSession,
        name: String,
        destination: SideForkDestination,
    ) -> Result<bool> {
        let Some(thread_id) = self.current_displayed_thread_id() else {
            return Ok(false);
        };
        let Some(parent_thread_id) = self.active_side_parent_thread_id() else {
            return Ok(false);
        };
        if self.side_threads.get(&thread_id).is_none_or(|side| {
            side.storage != crate::app_server_session::SideChatStorage::Temporary
        }) {
            return Ok(false);
        }
        if self.pending_side_summary.is_some()
            || self.active_turn_id_for_thread(thread_id).await.is_some()
            || self.chat_widget.is_user_turn_pending_or_running()
        {
            self.chat_widget.add_error_message(
                "Wait for the side conversation to finish before keeping it as a fork.".into(),
            );
            return Ok(false);
        }
        let persisted_thread = match app_server
            .thread_read(thread_id, /*include_turns*/ false)
            .await
        {
            Ok(thread) if !thread.ephemeral => thread,
            Ok(_) => {
                self.chat_widget.add_error_message(
                    "This older side conversation is ephemeral and cannot be kept as a fork."
                        .into(),
                );
                return Ok(false);
            }
            Err(err) => {
                self.chat_widget.add_error_message(format!(
                    "Failed to keep side conversation as a fork; it is still open: {err}"
                ));
                return Ok(false);
            }
        };
        if let Err(err) = app_server.thread_set_name(thread_id, name.clone()).await {
            self.chat_widget.add_error_message(format!(
                "Failed to name side conversation; it is still open in side mode: {err}"
            ));
            return Ok(false);
        }
        self.chat_widget.display_thread_name(thread_id, name);
        self.cancel_thread_title_generation(thread_id);
        let transition = AdditionalContextDeveloperFragment::new(
            "side_conversation_end".into(),
            "This conversation is now an ordinary fork. Side mode has ended; all previous side-only restrictions are superseded. Continue work requested after the fork boundary with normal capabilities under the current permissions and instructions. Inherited parent history remains reference-only unless a later user message explicitly asks you to continue a task from it.".into(),
        );
        if let Err(err) = app_server
            .thread_inject_items(
                thread_id,
                vec![ResponseItem::from(transition.render_fragment())],
            )
            .await
        {
            self.chat_widget.add_error_message(format!(
                "Failed to keep side conversation as a fork; it is still open: {err}"
            ));
            return Ok(false);
        }
        // Persistence started with thread/fork. No history export or replacement thread is needed.
        app_server.side_conversation_threads.remove(&thread_id);
        self.side_threads.remove(&thread_id);
        if let Some(channel) = self.thread_event_channels.get(&thread_id) {
            let mut store = channel.store.lock().await;
            if let Some(session) = store.session.as_mut() {
                session.forked_from_id = Some(parent_thread_id);
                session.daybreak_enabled = persisted_thread.daybreak_enabled.unwrap_or(false);
                self.chat_widget.keep_side_conversation_as_fork(session);
                if destination == SideForkDestination::Continue {
                    self.primary_session_configured = Some(session.clone());
                }
            }
        }
        if destination == SideForkDestination::Continue {
            self.primary_thread_id = Some(thread_id);
        }
        self.recap.reset_for_new_thread(Instant::now());
        self.sync_active_agent_label();
        Ok(true)
    }
}

#[cfg(test)]
#[path = "side_fork_tests.rs"]
mod tests;
