use super::*;
use codex_app_server_protocol::ThreadControlInputParams;
use codex_app_server_protocol::ThreadControlInputResponse;

pub(super) enum TurnStartBehavior {
    StartOrSteer,
    StartIfIdle,
}

impl TurnRequestProcessor {
    pub(crate) async fn thread_control_input(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadControlInputParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        if params.input.is_empty() {
            return Err(invalid_request("controller input must not be empty"));
        }
        validate_user_input_image_urls(&params.input)?;
        let turn_id = match params.expected_turn_id {
            Some(expected_turn_id) => {
                self.turn_steer_inner(
                    &request_id,
                    TurnSteerParams {
                        thread_id: params.thread_id,
                        client_user_message_id: None,
                        input: params.input,
                        responsesapi_client_metadata: None,
                        responsesapi_client_metadata_patch: None,
                        additional_context: None,
                        expected_turn_id,
                    },
                )
                .await?
                .turn_id
            }
            None => {
                self.turn_start_inner(
                    request_id,
                    TurnStartParams {
                        thread_id: params.thread_id,
                        input: params.input,
                        turn_trigger: Some("externalController".to_owned()),
                        ..Default::default()
                    },
                    /*app_server_client_name*/ None,
                    /*app_server_client_version*/ None,
                    TurnExecutionMode::Normal,
                    TurnStartBehavior::StartIfIdle,
                )
                .await?
                .turn
                .id
            }
        };
        Ok(Some(ThreadControlInputResponse { turn_id }.into()))
    }
}
