//! Archive the current chat before opening a fresh chat or exiting the TUI.

use super::*;
use crate::app_event::ArchiveMode;

impl App {
    pub(super) async fn archive_current_thread(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        mode: ArchiveMode,
    ) -> Result<AppRunControl> {
        let Some(thread_id) = self.active_thread_id.or(self.chat_widget.thread_id()) else {
            self.chat_widget
                .add_error_message("A thread must start before it can be archived.".to_string());
            return Ok(AppRunControl::Continue);
        };
        if self.side_threads.contains_key(&thread_id) {
            self.chat_widget.add_error_message(
                "'/archive' is unavailable in side conversations. Press Ctrl+C to return to the main thread first."
                    .to_string(),
            );
            return Ok(AppRunControl::Continue);
        }

        if !matches!(self.app_server_target, AppServerTarget::Embedded) {
            self.shutdown_side_threads(app_server).await;
            if !self.side_threads.is_empty() {
                return Ok(AppRunControl::Continue);
            }
        }

        let result = async {
            self.stop_voice_for_removed_thread(app_server, thread_id)
                .await?;
            app_server.thread_archive(thread_id).await
        }
        .await;
        Ok(match result {
            Ok(()) => {
                self.track_agents_overview_notification(&ServerNotification::ThreadArchived(
                    codex_app_server_protocol::ThreadArchivedNotification {
                        thread_id: thread_id.to_string(),
                    },
                ));
                self.discard_thread_local_state(thread_id).await;
                self.agents_overview.input_states.remove(&thread_id);
                self.agents_overview.dispatched_requests.remove(&thread_id);
                match mode {
                    ArchiveMode::NewChat => {
                        self.start_fresh_session(
                            tui, app_server, /*session_start_source*/ None,
                            /*initial_user_message*/ None, /*new_thread_name*/ None,
                        )
                        .await;
                        self.chat_widget.add_info_message(
                            "Archived previous chat.".to_string(),
                            /*hint*/ None,
                        );
                        tui.frame_requester().schedule_frame();
                        AppRunControl::Continue
                    }
                    ArchiveMode::Exit => AppRunControl::Exit(ExitReason::Archived(thread_id)),
                }
            }
            Err(err) => {
                self.chat_widget
                    .add_error_message(format!("Failed to archive current thread: {err}"));
                AppRunControl::Continue
            }
        })
    }
}
