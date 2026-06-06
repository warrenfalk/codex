//! Summary handoff for ephemeral side conversations.
//!
//! This module owns the close-prompt choices that either discard a side thread or
//! ask the side agent to produce a parent-thread summary before the side thread is
//! discarded.

use super::*;
use crate::app_event::SideConversationCloseChoice;
use codex_app_server_protocol::UserInput;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

const SIDE_SUMMARY_HEADING: &str = "Side conversation summary";
pub(super) const SIDE_SUMMARY_PROMPT: &str = r#"Summarize this side conversation for the parent thread.

Only summarize messages, findings, and decisions after the "Side conversation boundary." message. Treat all inherited parent-thread history before that boundary as context only; do not summarize it as side-conversation work.

Output only the summary text. Do not call tools, continue tasks, or modify workspace state. Include useful conclusions, relevant files or commands mentioned, and unresolved follow-ups."#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingSideSummary {
    pub(super) side_thread_id: ThreadId,
    pub(super) parent_thread_id: ThreadId,
    turn: PendingSideSummaryTurn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingSideSummaryTurn {
    AwaitingStart,
    Running(String),
}

impl App {
    pub(super) async fn handle_side_conversation_close_choice(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        choice: SideConversationCloseChoice,
    ) -> Result<()> {
        match choice {
            SideConversationCloseChoice::Cancel => {}
            SideConversationCloseChoice::Leave => {
                if self.pending_side_summary.is_some() {
                    self.chat_widget.add_error_message(
                        "A side conversation summary is already in progress.".to_string(),
                    );
                    return Ok(());
                }
                if let Some(parent_thread_id) = self.active_side_parent_thread_id() {
                    self.select_agent_thread_and_discard_side(tui, app_server, parent_thread_id)
                        .await?;
                }
            }
            SideConversationCloseChoice::Summarize => {
                let Some(side_thread_id) = self.current_displayed_thread_id() else {
                    return Ok(());
                };
                let Some(parent_thread_id) = self.active_side_parent_thread_id() else {
                    return Ok(());
                };
                self.start_side_summary(app_server, side_thread_id, parent_thread_id)
                    .await?;
            }
        }
        Ok(())
    }

    async fn start_side_summary(
        &mut self,
        app_server: &mut AppServerSession,
        side_thread_id: ThreadId,
        parent_thread_id: ThreadId,
    ) -> Result<()> {
        if self.pending_side_summary.is_some() {
            self.chat_widget.add_error_message(
                "A side conversation summary is already in progress.".to_string(),
            );
            return Ok(());
        }
        if self
            .active_turn_id_for_thread(side_thread_id)
            .await
            .is_some()
        {
            self.chat_widget.add_error_message(
                "Wait for the side conversation turn to finish before summarizing.".to_string(),
            );
            return Ok(());
        }
        let Some(op) = self.side_summary_user_turn() else {
            self.chat_widget.add_error_message(
                "Thread model is unavailable. Wait for the thread to finish syncing or choose a model before summarizing the side conversation."
                    .to_string(),
            );
            return Ok(());
        };

        self.pending_side_summary = Some(PendingSideSummary {
            side_thread_id,
            parent_thread_id,
            turn: PendingSideSummaryTurn::AwaitingStart,
        });
        if let Err(err) = self.submit_thread_op(app_server, side_thread_id, op).await {
            self.pending_side_summary = None;
            self.chat_widget
                .add_error_message(format!("Failed to summarize side conversation: {err}"));
        }
        Ok(())
    }

    pub(super) fn side_summary_user_turn(&self) -> Option<AppCommand> {
        let model = self.chat_widget.current_model().trim();
        if model.is_empty() {
            return None;
        }
        let config = self.chat_widget.config_ref();
        Some(AppCommand::user_turn(
            vec![UserInput::Text {
                text: SIDE_SUMMARY_PROMPT.to_string(),
                text_elements: Vec::new(),
            }],
            config.cwd.to_path_buf(),
            AskForApproval::from(config.permissions.approval_policy.value()),
            config.permissions.active_permission_profile(),
            model.to_string(),
            self.chat_widget.current_reasoning_effort(),
            /*summary*/ None,
            self.chat_widget.configured_service_tier().map(Some),
            /*final_output_json_schema*/ None,
            /*collaboration_mode*/ None,
            /*personality*/ None,
        ))
    }

    pub(super) fn note_side_summary_turn_started(
        &mut self,
        side_thread_id: ThreadId,
        turn_id: &str,
    ) {
        let Some(pending) = self.pending_side_summary.as_mut() else {
            return;
        };
        if pending.side_thread_id == side_thread_id {
            pending.turn = PendingSideSummaryTurn::Running(turn_id.to_string());
        }
    }

    pub(super) async fn handle_side_summary_turn_completed(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        side_thread_id: ThreadId,
        turn: Turn,
    ) -> Result<()> {
        let Some(pending) = self.pending_side_summary.clone() else {
            return Ok(());
        };
        if pending.side_thread_id != side_thread_id
            || !matches!(
                pending.turn,
                PendingSideSummaryTurn::Running(ref turn_id) if turn_id == &turn.id
            )
        {
            return Ok(());
        }

        match turn.status {
            TurnStatus::Completed => {
                let Some(summary) = self.side_summary_agent_text(side_thread_id, &turn).await
                else {
                    self.pending_side_summary = None;
                    self.chat_widget.add_error_message(
                        "Side conversation summary did not produce assistant text; the side conversation is still open."
                            .to_string(),
                    );
                    return Ok(());
                };
                self.save_completed_side_summary(tui, app_server, pending, &summary)
                    .await?;
            }
            TurnStatus::Interrupted => {
                self.pending_side_summary = None;
                self.chat_widget.add_error_message(
                    "Side conversation summary was interrupted; the side conversation is still open."
                        .to_string(),
                );
            }
            TurnStatus::Failed => {
                self.pending_side_summary = None;
                self.chat_widget.add_error_message(
                    "Side conversation summary failed; the side conversation is still open."
                        .to_string(),
                );
            }
            TurnStatus::InProgress => {}
        }
        Ok(())
    }

    async fn save_completed_side_summary(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        pending: PendingSideSummary,
        summary: &str,
    ) -> Result<()> {
        self.pending_side_summary = None;
        if let Err(err) = self
            .select_agent_thread(tui, app_server, pending.parent_thread_id)
            .await
        {
            self.chat_widget.add_error_message(format!(
                "Failed to return to the parent thread before saving the side conversation summary: {err}"
            ));
            return Ok(());
        }

        if let Err(err) = app_server
            .thread_inject_items(
                pending.parent_thread_id,
                vec![Self::side_summary_injection_item(summary)],
            )
            .await
        {
            self.chat_widget.add_error_message(format!(
                "Failed to save side conversation summary; the side conversation is still open: {err}"
            ));
            self.keep_side_thread_visible_after_cleanup_failure(
                tui,
                app_server,
                pending.side_thread_id,
            )
            .await;
            return Ok(());
        }

        if self
            .discard_side_thread(app_server, pending.side_thread_id)
            .await
        {
            self.surface_pending_inactive_thread_interactive_requests()
                .await;
        } else {
            self.keep_side_thread_visible_after_cleanup_failure(
                tui,
                app_server,
                pending.side_thread_id,
            )
            .await;
        }
        Ok(())
    }

    async fn side_summary_agent_text(
        &self,
        side_thread_id: ThreadId,
        turn: &Turn,
    ) -> Option<String> {
        if let Some(text) = turn.items.iter().rev().find_map(Self::agent_message_text) {
            return Some(text);
        }
        let channel = self.thread_event_channels.get(&side_thread_id)?;
        let store = channel.store.lock().await;
        store.buffer.iter().rev().find_map(|event| match event {
            ThreadBufferedEvent::Notification(ServerNotification::ItemCompleted(notification))
                if notification.turn_id == turn.id =>
            {
                Self::agent_message_text(&notification.item)
            }
            ThreadBufferedEvent::Notification(ServerNotification::TurnCompleted(notification))
                if notification.turn.id == turn.id =>
            {
                notification
                    .turn
                    .items
                    .iter()
                    .rev()
                    .find_map(Self::agent_message_text)
            }
            ThreadBufferedEvent::Request(_)
            | ThreadBufferedEvent::Notification(_)
            | ThreadBufferedEvent::HistoryEntryResponse(_)
            | ThreadBufferedEvent::FeedbackSubmission(_) => None,
        })
    }

    fn agent_message_text(item: &ThreadItem) -> Option<String> {
        match item {
            ThreadItem::AgentMessage { text, .. } => {
                let text = text.trim();
                (!text.is_empty()).then(|| text.to_string())
            }
            ThreadItem::UserMessage { .. }
            | ThreadItem::HookPrompt { .. }
            | ThreadItem::Reasoning { .. }
            | ThreadItem::CommandExecution { .. }
            | ThreadItem::FileChange { .. }
            | ThreadItem::McpToolCall { .. }
            | ThreadItem::DynamicToolCall { .. }
            | ThreadItem::CollabAgentToolCall { .. }
            | ThreadItem::WebSearch { .. }
            | ThreadItem::ImageView { .. }
            | ThreadItem::ImageGeneration { .. }
            | ThreadItem::EnteredReviewMode { .. }
            | ThreadItem::ExitedReviewMode { .. }
            | ThreadItem::ContextCompaction { .. }
            | ThreadItem::Plan { .. } => None,
        }
    }

    pub(super) fn side_summary_injection_item(summary: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: format!("{SIDE_SUMMARY_HEADING}\n\n{}", summary.trim()),
            }],
            phase: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn side_summary_injection_item_is_headed_assistant_note() {
        let item = App::side_summary_injection_item("  Findings from side chat.  ");
        let ResponseItem::Message { role, content, .. } = item else {
            panic!("expected summary injection to be a message");
        };
        assert_eq!(role, "assistant");
        let [ContentItem::OutputText { text }] = content.as_slice() else {
            panic!("expected output text summary");
        };
        assert_eq!(
            text,
            "Side conversation summary\n\nFindings from side chat."
        );
    }

    #[test]
    fn side_summary_agent_text_uses_trimmed_agent_messages_only() {
        let item = ThreadItem::AgentMessage {
            id: "agent-1".to_string(),
            text: "  final summary  ".to_string(),
            phase: None,
            memory_citation: None,
        };
        assert_eq!(
            App::agent_message_text(&item),
            Some("final summary".to_string())
        );

        let empty_item = ThreadItem::AgentMessage {
            id: "agent-2".to_string(),
            text: "   ".to_string(),
            phase: None,
            memory_citation: None,
        };
        assert_eq!(App::agent_message_text(&empty_item), None);

        let user_item = ThreadItem::UserMessage {
            id: "user-1".to_string(),
            client_id: None,
            content: Vec::new(),
        };
        assert_eq!(App::agent_message_text(&user_item), None);
    }
}
