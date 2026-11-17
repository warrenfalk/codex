//! Side mode belongs to this client session; the underlying thread is a durable fork.

use super::*;
use codex_app_server_protocol::AdditionalContextEntry;
use codex_app_server_protocol::AdditionalContextKind;

impl AppServerSession {
    pub(crate) async fn fork_side_thread(
        &mut self,
        local_settings: &LocalSettings,
        config: Config,
        thread_id: ThreadId,
        selected_profile: Option<&PermissionProfileSelection>,
        storage: SideChatStorage,
    ) -> Result<AppServerStartedThread> {
        let mut config = config;
        storage.configure_fork(&mut config);
        let temporary = storage == SideChatStorage::Temporary;
        let parent_name = if temporary && !config.ephemeral {
            match self.thread_read(thread_id, /*include_turns*/ false).await {
                Ok(parent) => {
                    config.ephemeral = parent.ephemeral;
                    parent.name
                }
                Err(error) => {
                    tracing::warn!(%thread_id, %error, "failed to read side fork parent name");
                    None
                }
            }
        } else {
            None
        };
        let persisted = temporary && !config.ephemeral;
        let mut started = self
            .fork_thread_at_with_presentation(
                local_settings,
                config,
                thread_id,
                /*last_turn_id*/ None,
                /*before_turn_id*/ None,
                ForkGoalContinuation::StartIfIdle,
                ForkPresentation::SideConversation(storage),
                selected_profile,
                ForkPermissionMode::InheritSaved,
                ForkConfigSource::Session,
            )
            .await?;
        let side_id = started.session.thread_id;
        if temporary {
            self.side_conversation_threads.insert(side_id);
        }
        if persisted {
            let name = match parent_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                Some(parent_name) => format!("(Side) {parent_name}"),
                None => format!("(Side) {side_id}"),
            };
            match self.thread_set_name(side_id, name.clone()).await {
                Ok(()) => started.session.thread_name = Some(name),
                // The fork already exists. Preserve access to it if naming fails.
                Err(error) => tracing::warn!(%side_id, %error, "failed to name side conversation"),
            }
        }
        Ok(started)
    }

    pub(super) fn side_turn_context(
        &self,
        thread_id: ThreadId,
        client_user_message_id: &str,
    ) -> Option<HashMap<String, AdditionalContextEntry>> {
        self.side_conversation_threads
            .contains(&thread_id)
            .then(|| {
                HashMap::from([(
                    "side_conversation".to_string(),
                    AdditionalContextEntry {
                        kind: AdditionalContextKind::Application,
                        name: None,
                        // The id ensures each turn receives its own scoped instruction, even when
                        // additional-context deduplication retained the preceding turn's value.
                        value: format!(
                            "Side turn {client_user_message_id}.\n\n{}",
                            crate::app::SIDE_DEVELOPER_INSTRUCTIONS
                        ),
                    },
                )])
            })
    }
}

#[cfg(test)]
#[path = "side_conversation_tests.rs"]
mod tests;
