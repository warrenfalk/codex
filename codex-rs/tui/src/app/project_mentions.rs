//! Keep accepted project roots with the owning thread, including background session caches.

use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;

impl App {
    pub(super) async fn record_project_roots(
        &mut self,
        thread_id: ThreadId,
        roots: Vec<AbsolutePathBuf>,
        added: &[AbsolutePathBuf],
    ) {
        if self.active_thread_id == Some(thread_id) {
            self.chat_widget.on_projects_attached(roots.clone(), added);
            if !self.app_server_target.uses_remote_workspace() {
                self.config.workspace_roots = roots.clone();
                self.config.permissions.set_workspace_roots(roots.clone());
            }
        }
        if self.primary_thread_id == Some(thread_id)
            && let Some(session) = self.primary_session_configured.as_mut()
        {
            session.runtime_workspace_roots = roots.clone();
        }
        if let Some(blank) = self.agents_overview.blank_sessions.get_mut(&thread_id) {
            blank.session.runtime_workspace_roots = roots.clone();
        }
        if let Some(channel) = self.thread_event_channels.get(&thread_id) {
            let mut store = channel.store.lock().await;
            if let Some(session) = store.session.as_mut() {
                session.runtime_workspace_roots = roots;
            }
        }
    }
}
