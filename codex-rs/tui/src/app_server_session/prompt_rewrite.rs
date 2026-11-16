//! Ephemeral rewrite helpers disable configured hooks before creation.

use super::*;

impl AppServerSession {
    pub(crate) async fn create_prompt_rewrite_thread(
        &mut self,
        config: &Config,
        parent_thread_id: ThreadId,
        primary_thread_id: Option<ThreadId>,
    ) -> Result<ThreadId> {
        let overrides = HashMap::from([(
            "features".to_string(),
            serde_json::json!({ (codex_features::Feature::CodexHooks.key()): false }),
        )]);
        let request_id = self.next_request_id();
        let fork: Result<ThreadForkResponse> = self
            .request_handle
            .request_typed(ClientRequest::ThreadFork {
                request_id,
                params: ThreadForkParams {
                    thread_id: parent_thread_id.to_string(),
                    model: config.model.clone(),
                    service_tier: service_tier_override_from_config(config),
                    developer_instructions: config.developer_instructions.clone(),
                    config: Some(overrides.clone()),
                    ephemeral: true,
                    exclude_turns: true,
                    thread_source: Some(ThreadSource::Feature("prompt_rewrite".to_string())),
                    ..Default::default()
                },
            })
            .await
            .map_err(|err| bootstrap_request_error("rewrite thread/fork failed", err));
        match fork {
            Ok(response) => Ok(ThreadId::from_string(&response.thread.id)?),
            Err(error)
                if primary_thread_id == Some(parent_thread_id)
                    && error.chain().any(|cause| {
                        let message = cause.to_string();
                        message.contains("no rollout found for thread id")
                            || message
                                .contains("includeTurns is unavailable before first user message")
                    }) =>
            {
                let request_id = self.next_request_id();
                let response: ThreadStartResponse = self
                    .request_handle
                    .request_typed(ClientRequest::ThreadStart {
                        request_id,
                        params: ThreadStartParams {
                            model: config.model.clone(),
                            model_provider: Some(config.model_provider_id.clone()),
                            service_tier: service_tier_override_from_config(config),
                            developer_instructions: config.developer_instructions.clone(),
                            config: Some(overrides),
                            environments: Some(Vec::new()),
                            ephemeral: Some(true),
                            thread_source: Some(ThreadSource::Feature(
                                "prompt_rewrite".to_string(),
                            )),
                            ..Default::default()
                        },
                    })
                    .await
                    .map_err(|err| bootstrap_request_error("rewrite thread/start failed", err))?;
                Ok(ThreadId::from_string(&response.thread.id)?)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn turn_start_model_only(
        &mut self,
        thread_id: ThreadId,
        items: Vec<UserInput>,
        model: String,
        effort: Option<codex_protocol::openai_models::ReasoningEffort>,
        output_schema: Option<serde_json::Value>,
    ) -> Result<TurnStartModelOnlyResponse> {
        let request_id = self.next_request_id();
        self.request_handle
            .request_typed(ClientRequest::TurnStartModelOnly {
                request_id,
                params: TurnStartModelOnlyParams {
                    thread_id: thread_id.to_string(),
                    input: items,
                    model,
                    effort,
                    output_schema,
                },
            })
            .await
            .wrap_err("turn/startModelOnly failed in TUI")
    }
}
