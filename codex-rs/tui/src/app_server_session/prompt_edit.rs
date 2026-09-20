//! Replace a conversation suffix using the operation supported by its history format.

use super::AppServerSession;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadRevertParams;
use codex_app_server_protocol::ThreadRevertResponse;
use codex_app_server_protocol::ThreadRollbackParams;
use codex_app_server_protocol::ThreadRollbackResponse;
use codex_protocol::ThreadId;
use color_eyre::eyre::ContextCompat;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;

impl AppServerSession {
    pub(crate) async fn revert_thread_before(
        &mut self,
        thread_id: ThreadId,
        before_turn_id: String,
    ) -> Result<Thread> {
        let thread = self.thread_read(thread_id, /*include_turns*/ false).await?;
        match thread.history_mode {
            ThreadHistoryMode::Paginated => {
                let request_id = self.next_request_id();
                let response: ThreadRevertResponse = self
                    .client
                    .request_typed(ClientRequest::ThreadRevert {
                        request_id,
                        params: ThreadRevertParams {
                            thread_id: thread_id.to_string(),
                            before_turn_id,
                        },
                    })
                    .await
                    .wrap_err("thread/revert failed")?;
                self.history_pagination.remove(&thread_id);
                Ok(response.thread)
            }
            ThreadHistoryMode::Legacy => {
                let thread = self.thread_read(thread_id, /*include_turns*/ true).await?;
                let selected = thread
                    .turns
                    .iter()
                    .position(|turn| turn.id == before_turn_id)
                    .wrap_err("the selected turn is no longer in the conversation")?;
                // Legacy rollback counts user inputs, including steers, rather than API Turns.
                let num_turns = thread.turns[selected..]
                    .iter()
                    .flat_map(|turn| &turn.items)
                    .filter(|item| matches!(item, ThreadItem::UserMessage { .. }))
                    .count();
                let num_turns = u32::try_from(num_turns)?;
                let request_id = self.next_request_id();
                let response: ThreadRollbackResponse = self
                    .client
                    .request_typed(ClientRequest::ThreadRollback {
                        request_id,
                        params: ThreadRollbackParams {
                            thread_id: thread_id.to_string(),
                            num_turns,
                        },
                    })
                    .await
                    .wrap_err("thread/rollback failed")?;
                Ok(response.thread)
            }
        }
    }
}
