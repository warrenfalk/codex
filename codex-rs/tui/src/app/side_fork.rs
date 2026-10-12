//! Keeping a persisted side conversation as an ordinary fork.

use super::*;
use codex_context_fragments::AdditionalContextDeveloperFragment;
use codex_context_fragments::ContextualUserFragment;
use codex_protocol::models::ResponseItem;

impl App {
    pub(super) async fn keep_side_conversation_as_fork(
        &mut self,
        app_server: &mut AppServerSession,
    ) -> Result<()> {
        let Some(thread_id) = self.current_displayed_thread_id() else {
            return Ok(());
        };
        let Some(parent_thread_id) = self.active_side_parent_thread_id() else {
            return Ok(());
        };
        if self.pending_side_summary.is_some()
            || self.active_turn_id_for_thread(thread_id).await.is_some()
            || self.chat_widget.is_agent_turn_running()
        {
            self.chat_widget.add_error_message(
                "Wait for the side conversation to finish before keeping it as a fork.".into(),
            );
            return Ok(());
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
                return Ok(());
            }
            Err(err) => {
                self.chat_widget.add_error_message(format!(
                    "Failed to keep side conversation as a fork; it is still open: {err}"
                ));
                return Ok(());
            }
        };
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
            return Ok(());
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
                self.primary_session_configured = Some(session.clone());
            }
        }
        self.primary_thread_id = Some(thread_id);
        self.recap.reset_for_new_thread(Instant::now());
        self.sync_active_agent_label();
        Ok(())
    }
}

#[cfg(test)]
#[path = "side_fork_tests.rs"]
mod tests;
