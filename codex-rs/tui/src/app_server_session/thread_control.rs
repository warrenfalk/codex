use super::AppServerSession;
use codex_app_server_protocol::*;
use codex_protocol::ThreadId;
use color_eyre::Result;

impl AppServerSession {
    pub(crate) async fn thread_control_read(
        &mut self,
        thread_id: ThreadId,
    ) -> Result<ThreadControlState> {
        let request_id = self.next_request_id();
        let response: ThreadControlReadResponse = self
            .client
            .request_typed(ClientRequest::ThreadControlRead {
                request_id,
                params: ThreadControlReadParams {
                    thread_id: thread_id.to_string(),
                },
            })
            .await?;
        Ok(response.state)
    }

    pub(crate) async fn thread_control_update(
        &mut self,
        thread_id: ThreadId,
        action: ThreadControlAction,
    ) -> Result<ThreadControlState> {
        let request_id = self.next_request_id();
        let response: ThreadControlUpdateResponse = self
            .client
            .request_typed(ClientRequest::ThreadControlUpdate {
                request_id,
                params: ThreadControlUpdateParams {
                    thread_id: thread_id.to_string(),
                    action,
                },
            })
            .await?;
        Ok(response.state)
    }
}
