//! Dashboard scheduling and activity presentation around the shared agents model.

use super::App;
use super::agents_overview::AGENTS_OVERVIEW_VIEW_ID;
use super::app_server_event_targets::ServerNotificationThreadTarget;
use super::app_server_event_targets::server_notification_thread_target;
use crate::AppServerTarget;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use codex_app_server_protocol::ServerNotification;

impl App {
    pub(super) fn track_agents_overview_notification(
        &mut self,
        notification: &ServerNotification,
    ) -> bool {
        let ServerNotificationThreadTarget::Thread(id) =
            server_notification_thread_target(notification)
        else {
            return false;
        };
        self.track_agents_overview_activity(id, notification);
        let changed = self.agents_overview.model.observe(notification);
        match notification {
            ServerNotification::ThreadArchived(_)
            | ServerNotification::ThreadDeleted(_)
            | ServerNotification::ThreadClosed(_) => {
                self.agents_overview.activity.remove(&id);
            }
            ServerNotification::ThreadReverted(_) => {
                self.agents_overview.activity.remove(&id);
                self.repaint_agents_overview();
            }
            _ => {}
        }
        changed
    }

    pub(super) fn refresh_agents_overview_threads(&mut self, app_server: &AppServerSession) {
        self.agents_overview
            .model
            .refresh_thread_ids
            .extend(self.agents_overview.model.threads.keys());
        self.start_agents_overview_refresh(app_server);
    }

    pub(super) fn refresh_changed_agents_overview_threads(
        &mut self,
        app_server: &AppServerSession,
    ) {
        if self
            .chat_widget
            .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
            .is_none()
            || (!self.agents_overview.model.initialized
                && self.agents_overview.model.request_id.is_none())
        {
            return;
        }
        self.start_agents_overview_refresh(app_server);
    }

    fn start_agents_overview_refresh(&mut self, app_server: &AppServerSession) {
        let visible = self
            .chat_widget
            .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
            .is_some();
        if !visible
            && (self.agents_overview.model.initialized
                || matches!(self.app_server_target, AppServerTarget::Embedded))
        {
            return;
        }
        let Some(request) = self.agents_overview.model.begin_refresh() else {
            return;
        };
        let request_id = request.id;
        let handle = app_server.request_handle();
        let app_event_tx = self.app_event_tx.clone();
        let refresh_task = tokio::spawn(async move {
            let result = request
                .fetch(handle)
                .await
                .map_err(|error| error.to_string());
            app_event_tx.send(AppEvent::AgentsOverviewThreadsLoaded { request_id, result });
        });
        self.agents_overview.refresh_task = Some(refresh_task.abort_handle());
    }
}
