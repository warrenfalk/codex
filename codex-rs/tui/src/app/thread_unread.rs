use super::App;
use crate::app_server_session::AppServerSession;
use codex_protocol::ThreadId;

impl App {
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
