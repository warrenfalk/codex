use super::AppServerSession;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadUnreadSetParams;
use codex_app_server_protocol::ThreadUnreadSetResponse;
use codex_protocol::ThreadId;
use color_eyre::Result;

impl AppServerSession {
    pub(crate) async fn thread_unread_set(
        &mut self,
        thread_id: ThreadId,
        unread: bool,
    ) -> Result<bool> {
        let request_id = self.next_request_id();
        let response: ThreadUnreadSetResponse = self
            .client
            .request_typed(ClientRequest::ThreadUnreadSet {
                request_id,
                params: ThreadUnreadSetParams {
                    thread_id: thread_id.to_string(),
                    unread,
                },
            })
            .await?;
        Ok(response.unread)
    }
}
