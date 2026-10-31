use super::*;
use codex_app_server_protocol::ThreadControlState;

impl ChatWidget {
    pub(super) fn dispatch_control_command(&mut self, command: &str) {
        if let Some(thread_id) = self.thread_id {
            self.app_event_tx.send(AppEvent::ThreadControlCommand {
                thread_id,
                command: command.to_owned(),
            });
        }
    }

    pub(super) fn revoke_control_on_submission(&self) {
        if let Some(thread_id) = self.thread_id {
            self.app_event_tx
                .send(AppEvent::ThreadControlUserInput { thread_id });
        }
    }

    pub(crate) fn set_thread_control_state(&mut self, state: Option<ThreadControlState>) {
        self.bottom_pane.set_thread_control_state(state);
    }
}
