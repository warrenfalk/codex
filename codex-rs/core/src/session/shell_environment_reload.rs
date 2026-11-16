use super::session::Session;
use super::session::SessionConfiguration;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use std::sync::Arc;

#[cfg(test)]
#[path = "shell_environment_reload_tests.rs"]
mod tests;

impl Session {
    pub(super) async fn reload_shell_environment(
        &self,
        configuration: &mut SessionConfiguration,
        sub_id: &str,
    ) {
        let expected = Arc::clone(&configuration.original_config_do_not_use);
        let mut next = expected.as_ref().clone();
        let policy = match next
            .shell_environment_reload
            .reload(
                codex_exec_server::LOCAL_FS.as_ref(),
                &next.codex_home,
                configuration.cwd().clone(),
                &next.config_layer_stack,
            )
            .await
        {
            Ok(Some(policy)) => policy,
            Ok(None) => return,
            Err(_) => {
                // Config parse errors can contain literal credentials. Do not include them
                // in events, tracing, or model-visible context.
                self.send_event_raw(Event {
                    id: sub_id.to_string(),
                    msg: EventMsg::Warning(WarningEvent {
                        message: "Could not reload shell environment configuration; keeping the previous environment policy for this turn.".to_string(),
                    }),
                })
                .await;
                return;
            }
        };
        next.permissions.shell_environment_policy = policy.clone();
        let next = Arc::new(next);
        {
            let mut state = self.state.lock().await;
            let saved = &mut state.session_configuration;
            if Arc::ptr_eq(&saved.original_config_do_not_use, &expected) {
                saved.shell_environment_policy = policy.clone();
                saved.original_config_do_not_use = Arc::clone(&next);
            }
        }
        configuration.shell_environment_policy = policy;
        configuration.original_config_do_not_use = next;
    }
}
