use super::*;
use codex_app_server_protocol::ThreadControlReadResponse;
use codex_app_server_protocol::ThreadControlReleaseResponse;
use codex_app_server_protocol::ThreadControlRequestResponse;
use codex_app_server_protocol::ThreadControlUpdateResponse;
use codex_app_server_protocol::TurnInterruptParams;

#[cfg(test)]
#[path = "thread_control_tests.rs"]
mod tests;

impl MessageProcessor {
    pub(super) async fn handle_thread_control(
        &self,
        request: ClientRequest,
        request_id: ConnectionRequestId,
        session: &ConnectionSessionState,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        match request {
            ClientRequest::ControllerRegister { .. } => {
                unreachable!("registration precedes dispatch")
            }
            ClientRequest::ThreadControlRead { params, .. } => {
                let (id, entry) = self.thread_control.entry(&params.thread_id).await?;
                Ok(Some(
                    ThreadControlReadResponse {
                        state: self.thread_control.snapshot(id, &entry).await,
                    }
                    .into(),
                ))
            }
            ClientRequest::ThreadControlRequest { params, .. } => {
                let caller = session
                    .controller_id()
                    .ok_or_else(|| invalid_request("register an external controller first"))?;
                Ok(Some(
                    ThreadControlRequestResponse {
                        state: self.thread_control.request(caller, params).await?,
                    }
                    .into(),
                ))
            }
            ClientRequest::ThreadControlUpdate { params, .. } => Ok(Some(
                ThreadControlUpdateResponse {
                    state: self.thread_control.update(params).await?,
                }
                .into(),
            )),
            ClientRequest::ThreadControlRelease { params, .. } => {
                let caller = session
                    .controller_id()
                    .ok_or_else(|| invalid_request("register an external controller first"))?;
                Ok(Some(
                    ThreadControlReleaseResponse {
                        state: self.thread_control.release(caller, params).await?,
                    }
                    .into(),
                ))
            }
            ClientRequest::ThreadControlInput { params, .. } => {
                if session.controller_id().is_none() {
                    return Err(invalid_request("register an external controller first"));
                }
                request_future::boxed(|| {
                    self.turn_processor.thread_control_input(request_id, params)
                })
                .await
            }
            ClientRequest::ThreadControlInterrupt { params, .. } => {
                if session.controller_id().is_none() || params.turn_id.is_empty() {
                    return Err(invalid_request(
                        "a controller and explicit turnId are required",
                    ));
                }
                self.turn_processor
                    .turn_interrupt(
                        &request_id,
                        TurnInterruptParams {
                            thread_id: params.thread_id,
                            turn_id: params.turn_id,
                        },
                    )
                    .await
            }
            _ => unreachable!("only thread-control methods are routed here"),
        }
    }
}
