//! Side mode belongs to this client session; the underlying thread is a durable fork.

use super::*;
use codex_app_server_protocol::AdditionalContextEntry;
use codex_app_server_protocol::AdditionalContextKind;

impl AppServerSession {
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
