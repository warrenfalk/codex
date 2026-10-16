//! One-shot acknowledgement after sustained terminal focus and the stopped-session indicator.

use super::App;
use super::agents_overview::AGENTS_OVERVIEW_VIEW_ID;
use crate::app_server_session::AppServerSession;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use codex_app_server_protocol::ThreadStatus;
use codex_protocol::ThreadId;
use std::collections::HashSet;
use std::time::Duration;

const FOCUS_ACK_DELAY: Duration = Duration::from_secs(/*secs*/ 1);

impl App {
    pub(super) async fn handle_unread_focus(
        &mut self,
        tui: &Tui,
        app_server: &mut AppServerSession,
        event: &TuiEvent,
    ) {
        match event {
            TuiEvent::FocusGained => {
                self.pending_focus_ack = self
                    .current_displayed_thread_id()
                    .filter(|_| {
                        !self.chat_widget.side_conversation_active()
                            && self
                                .chat_widget
                                .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
                                .is_none()
                    })
                    .map(|id| (id, tokio::time::Instant::now() + FOCUS_ACK_DELAY));
                if self.pending_focus_ack.is_some() {
                    tui.frame_requester().schedule_frame_in(FOCUS_ACK_DELAY);
                }
            }
            TuiEvent::FocusLost | TuiEvent::Resume => self.pending_focus_ack = None,
            TuiEvent::Draw => {
                let Some((thread_id, deadline)) = self.pending_focus_ack else {
                    return;
                };
                if tokio::time::Instant::now() < deadline {
                    return;
                }
                // Consume the attempt even if the server ignores it while work is active.
                self.pending_focus_ack = None;
                if tui.is_terminal_focused()
                    && !self.reconnect.offline
                    && self.current_displayed_thread_id() == Some(thread_id)
                    && !self.chat_widget.side_conversation_active()
                    && self
                        .chat_widget
                        .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
                        .is_none()
                {
                    self.acknowledge_opened_thread(app_server, thread_id).await;
                }
            }
            TuiEvent::Key(_) | TuiEvent::Mouse(_) | TuiEvent::Paste(_) | TuiEvent::Resize(_) => {}
        }
    }

    pub(super) fn sync_thread_unread_indicator(&mut self) {
        let thread_id = self.current_displayed_thread_id();
        let unread = thread_id
            .and_then(|id| self.agents_overview.threads.get(&id))
            .and_then(Option::as_ref)
            .is_some_and(|thread| thread.unread && !thread.ephemeral);
        let active_family = unread
            && self
                .agents_overview
                .threads
                .values()
                .flatten()
                .any(|thread| {
                    if !matches!(thread.status, ThreadStatus::Active { .. }) {
                        return false;
                    }
                    let mut current = Some(thread);
                    let mut seen = HashSet::new();
                    while let Some(thread) = current {
                        let Ok(id) = ThreadId::from_string(&thread.id) else {
                            break;
                        };
                        if Some(id) == thread_id {
                            return true;
                        }
                        if !seen.insert(id) {
                            break;
                        }
                        current = crate::agents_list::parent_id(thread)
                            .and_then(|parent| ThreadId::from_string(&parent).ok())
                            .and_then(|id| self.agents_overview.threads.get(&id))
                            .and_then(Option::as_ref);
                    }
                    false
                });
        self.chat_widget
            .set_unread_indicator(unread && !active_family);
    }

    pub(super) async fn acknowledge_opened_thread(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        match app_server
            .thread_unread_set(thread_id, /*unread*/ false)
            .await
        {
            Ok(unread) => {
                if let Some(thread) = self
                    .agents_overview
                    .threads
                    .get_mut(&thread_id)
                    .and_then(Option::as_mut)
                {
                    thread.unread = unread;
                }
                self.sync_thread_unread_indicator();
                self.repaint_agents_overview();
            }
            Err(error) => tracing::warn!(%error, "failed to acknowledge opened session"),
        }
    }

    pub(super) async fn set_thread_unread(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        unread: bool,
    ) {
        match app_server.thread_unread_set(thread_id, unread).await {
            Ok(current) => {
                if let Some(thread) = self
                    .agents_overview
                    .threads
                    .get_mut(&thread_id)
                    .and_then(Option::as_mut)
                {
                    thread.unread = current;
                }
                self.sync_thread_unread_indicator();
                let message = if unread {
                    "Session marked unread."
                } else if current {
                    "This session has active work. Acknowledge again after it stops."
                } else {
                    "Session acknowledged."
                };
                self.chat_widget
                    .add_info_message(message.to_string(), /*hint*/ None);
            }
            Err(err) => self
                .chat_widget
                .add_error_message(format!("Failed to update unread state: {err}")),
        }
    }
}
