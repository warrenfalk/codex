use super::*;
use codex_app_server_protocol::ThreadControlAction;
use codex_app_server_protocol::ThreadControlRequestStatus;
use codex_app_server_protocol::ThreadControlState;

#[derive(Default)]
pub(super) struct ThreadControlUi {
    states: HashMap<ThreadId, ThreadControlState>,
    loaded: Option<ThreadId>,
}

impl App {
    pub(super) fn receive_thread_control(&mut self, state: ThreadControlState) {
        let Ok(id) = ThreadId::from_string(&state.thread_id) else {
            return;
        };
        let previous = self.thread_control.states.insert(id, state.clone());
        if let Some(request) = state
            .request
            .as_ref()
            .filter(|r| r.status == ThreadControlRequestStatus::Pending)
            && previous
                .as_ref()
                .and_then(|s| s.request.as_ref())
                .is_none_or(|r| {
                    r.id != request.id || r.status != ThreadControlRequestStatus::Pending
                })
        {
            self.chat_widget
                .notify(Notification::ThreadControlRequested {
                    controller_id: request.controller_id.clone(),
                });
        }
        if self.current_displayed_thread_id() == Some(id) {
            self.chat_widget.set_thread_control_state(Some(state));
        }
    }

    pub(super) async fn sync_thread_control_ui(
        &mut self,
        server: &mut AppServerSession,
        event: &TuiEvent,
    ) {
        if !matches!(event, TuiEvent::Draw) || self.reconnect.offline {
            return;
        }
        let id = self.current_displayed_thread_id();
        if self.thread_control.loaded != id {
            self.thread_control.loaded = id;
            self.chat_widget.set_thread_control_state(
                id.and_then(|id| self.thread_control.states.get(&id).cloned()),
            );
            if let Some(id) = id {
                match server.thread_control_read(id).await {
                    Ok(state) => self.receive_thread_control(state),
                    Err(error) => tracing::debug!(?error, "thread control state unavailable"),
                }
            }
        }
    }

    pub(super) fn reset_thread_control_ui(&mut self) {
        self.thread_control = ThreadControlUi::default();
        self.chat_widget.set_thread_control_state(None);
    }

    pub(super) async fn thread_control_command(
        &mut self,
        server: &mut AppServerSession,
        id: ThreadId,
        command: &str,
    ) {
        let command = command.trim();
        let pending_id = self
            .thread_control
            .states
            .get(&id)
            .and_then(|s| s.request.as_ref())
            .filter(|r| r.status == ThreadControlRequestStatus::Pending)
            .map(|r| r.id.clone());
        let action = match command {
            "approve" => pending_id.map(|request_id| ThreadControlAction::Approve { request_id }),
            "deny" => pending_id.map(|request_id| ThreadControlAction::Deny { request_id }),
            "revoke" => Some(ThreadControlAction::Revoke),
            "lock" => Some(ThreadControlAction::Lock),
            "unlock" => Some(ThreadControlAction::Unlock),
            "" => {
                match server.thread_control_read(id).await {
                    Ok(state) => {
                        let summary = format!(
                            "External control: {}. /control approve|deny|revoke|lock|unlock|grant <agent>",
                            if state.locked {
                                "locked"
                            } else if state.grant.is_some() {
                                "granted"
                            } else {
                                "available"
                            }
                        );
                        self.receive_thread_control(state);
                        self.chat_widget.add_info_message(summary, /*hint*/ None);
                    }
                    Err(error) => self
                        .chat_widget
                        .add_error_message(format!("Cannot read thread control: {error}")),
                }
                return;
            }
            _ => command
                .strip_prefix("grant ")
                .map(|id| ThreadControlAction::Grant {
                    controller_id: id.trim().to_owned(),
                }),
        };
        let Some(action) = action else {
            self.chat_widget.add_error_message("Use /control approve|deny (pending request required), revoke, lock, unlock, or grant <agent>.".to_owned());
            return;
        };
        match server.thread_control_update(id, action).await {
            Ok(state) => self.receive_thread_control(state),
            Err(error) => self
                .chat_widget
                .add_error_message(format!("Cannot update thread control: {error}")),
        }
    }

    pub(super) async fn revoke_control_for_input(
        &mut self,
        server: &mut AppServerSession,
        id: ThreadId,
    ) {
        match server
            .thread_control_update(id, ThreadControlAction::Revoke)
            .await
        {
            Ok(state) => self.receive_thread_control(state),
            Err(error) => tracing::warn!(?error, "cannot revoke external control for queued input"),
        }
    }
}
