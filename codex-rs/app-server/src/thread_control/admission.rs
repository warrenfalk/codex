use super::*;
use crate::message_processor::ConnectionSessionState;

/// Keep the bridge's authority narrower than the normal trusted app-server API.
pub(crate) fn validate_controller_request(
    request: &ClientRequest,
) -> Result<(), JSONRPCErrorError> {
    match request {
        ClientRequest::ThreadControlRead { .. }
        | ClientRequest::ThreadControlRequest { .. }
        | ClientRequest::ThreadControlRelease { .. }
        | ClientRequest::ThreadControlInput { .. }
        | ClientRequest::ThreadControlInterrupt { .. }
        | ClientRequest::EventFirehose { .. }
        | ClientRequest::ThreadList { .. }
        | ClientRequest::ThreadLoadedList { .. }
        | ClientRequest::ThreadRead { .. }
        | ClientRequest::ThreadTurnsList { .. }
        | ClientRequest::ThreadItemsList { .. }
        | ClientRequest::ThreadItemsRead { .. }
        | ClientRequest::ThreadUnsubscribe { .. } => Ok(()),
        ClientRequest::ThreadResume { params, .. } => {
            let allowed = ThreadResumeParams {
                thread_id: params.thread_id.clone(),
                exclude_turns: params.exclude_turns,
                initial_turns_page: params.initial_turns_page.clone(),
                ..Default::default()
            };
            if *params != allowed {
                return Err(invalid_request(
                    "controllers may resume by threadId without configuration overrides",
                ));
            }
            Ok(())
        }
        _ => Err(invalid_request(
            "method is not available to external controllers",
        )),
    }
}

impl ThreadControlService {
    /// Revoke at receipt, before this user's input can wait behind serialized work.
    pub(crate) async fn revoke_for_input(
        &self,
        session: &ConnectionSessionState,
        request: &ClientRequest,
    ) -> Result<(), JSONRPCErrorError> {
        if session.controller_id().is_none()
            && let Some((id, mut entry)) = self.human_entry(request).await?
            && entry.revoke()
        {
            self.publish(id, &entry).await;
        }
        Ok(())
    }

    pub(crate) async fn admit(
        &self,
        session: &ConnectionSessionState,
        request: &ClientRequest,
    ) -> Result<Option<OwnedMutexGuard<ControlEntry>>, JSONRPCErrorError> {
        if let Some(controller_id) = session.controller_id() {
            let (thread_id, grant_id) = match request {
                ClientRequest::ThreadControlInput { params, .. } => {
                    (&params.thread_id, &params.grant_id)
                }
                ClientRequest::ThreadControlInterrupt { params, .. } => {
                    (&params.thread_id, &params.grant_id)
                }
                _ => return Ok(None),
            };
            let (_, entry) = self.entry(thread_id).await?;
            super::requests::validate_grant(&entry, controller_id, grant_id)?;
            return Ok(Some(entry));
        }
        let Some((id, mut entry)) = self.human_entry(request).await? else {
            return Ok(None);
        };
        if entry.revoke() {
            self.publish(id, &entry).await;
        }
        Ok(Some(entry))
    }

    async fn human_entry(
        &self,
        request: &ClientRequest,
    ) -> Result<Option<(ThreadId, OwnedMutexGuard<ControlEntry>)>, JSONRPCErrorError> {
        let Some(thread_id) = human_input_thread_id(request) else {
            return Ok(None);
        };
        let Ok(id) = ThreadId::from_string(thread_id) else {
            return Ok(None);
        };
        // Leave invalid/unloaded-thread errors to the existing input handler.
        if self.threads.get_thread(id).await.is_err() {
            return Ok(None);
        }
        self.entry(thread_id).await.map(Some)
    }
}

fn human_input_thread_id(request: &ClientRequest) -> Option<&str> {
    let thread_id = match request {
        ClientRequest::TurnStart { params, .. } if !params.input.is_empty() => &params.thread_id,
        ClientRequest::TurnSteer { params, .. } if !params.input.is_empty() => &params.thread_id,
        ClientRequest::ThreadQueueAdd { params, .. } => &params.thread_id,
        ClientRequest::ThreadQueueStart { params, .. } => &params.thread_id,
        ClientRequest::ReviewStart { params, .. } => &params.thread_id,
        ClientRequest::ThreadShellCommand { params, .. } => &params.thread_id,
        ClientRequest::ThreadCompactStart { params, .. } => &params.thread_id,
        ClientRequest::ThreadRealtimeAppendText { params, .. } => &params.thread_id,
        _ => return None,
    };
    Some(thread_id)
}
