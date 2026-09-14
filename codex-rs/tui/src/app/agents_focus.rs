use super::*;
use crate::agents_focus::Action;
use crate::agents_focus::Endpoint;
use crate::agents_focus::PendingRequest;
use crate::agents_focus::Reply;
use crate::agents_focus::kitty;

impl App {
    pub(super) fn matches_focus_session(&self, endpoint: &Endpoint, session_id: &str) -> bool {
        let attached_endpoint = match &self.app_server_target {
            AppServerTarget::Embedded => return false,
            AppServerTarget::LocalDaemon { endpoint } | AppServerTarget::Remote { endpoint } => {
                endpoint
            }
        };
        if Endpoint::from(attached_endpoint) != *endpoint {
            return false;
        }
        let mut thread_id = self.primary_thread_id;
        let mut seen = HashSet::new();
        while let Some(id) = thread_id {
            if !seen.insert(id) {
                break;
            }
            if id.to_string() == session_id {
                return true;
            }
            thread_id = self
                .agents_overview
                .model
                .threads
                .get(&id)
                .and_then(Option::as_ref)
                .and_then(crate::agents_list::parent_id)
                .and_then(|parent| ThreadId::from_string(&parent).ok());
        }
        false
    }

    pub(super) async fn handle_agents_focus(
        &mut self,
        tui: &mut tui::Tui,
        pending: PendingRequest,
    ) {
        let PendingRequest {
            request,
            mut response,
        } = pending;
        if response.is_closed() {
            return;
        }
        let reply = if !self.matches_focus_session(&request.endpoint, &request.session_id) {
            Reply::NoMatch
        } else {
            match kitty::command(std::env::var_os) {
                Err(error) => Reply::Unavailable(error.to_string()),
                Ok(command) => match request.action {
                    Action::Probe => Reply::Ready,
                    Action::Focus => {
                        // Kitten may use the controlling TTY. Stop crossterm
                        // from consuming its reply; always restore input.
                        tui.pause_events();
                        let result = tokio::select! {
                            result = kitty::focus(command) => Some(result),
                            _ = response.closed() => None,
                        };
                        tui.resume_events();
                        match result {
                            Some(Ok(())) => Reply::Focused,
                            Some(Err(error)) => Reply::Unavailable(format!("{error:#}")),
                            None => return,
                        }
                    }
                },
            }
        };
        let _ = response.send(reply);
    }
}
