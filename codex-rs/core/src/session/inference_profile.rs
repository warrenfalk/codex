use super::*;
use crate::config::ConstraintError;
use crate::config::ConstraintResult;
use codex_inference_profiles::InferenceProfile;
use codex_model_provider::ProviderCapabilities;
use codex_model_provider::RemoteCompactionSupport;
use codex_model_provider::SharedModelProvider;
use codex_model_provider::create_model_provider;
use codex_model_provider::create_model_provider_with_capabilities;

pub(super) fn create_profile_model_provider(
    provider: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
    inference_profile: Option<InferenceProfile>,
) -> SharedModelProvider {
    match inference_profile {
        Some(InferenceProfile::KimiK3) => create_model_provider_with_capabilities(
            provider,
            auth_manager,
            ProviderCapabilities {
                namespace_tools: true,
                image_generation: false,
                web_search: false,
                external_web_access: false,
                remote_compaction: RemoteCompactionSupport::Unsupported,
            },
        ),
        None => create_model_provider(provider, auth_manager),
    }
}

impl Session {
    pub(crate) async fn validate_inference_profile_settings(
        &self,
        updates: &SessionSettingsUpdate,
    ) -> ConstraintResult<()> {
        let state = self.state.lock().await;
        let Ok(updated) = self.apply_session_settings(&state.session_configuration, updates) else {
            return Ok(());
        };
        self.validate_inference_profile_configuration(&updated)
    }

    pub(super) fn validate_inference_profile_configuration(
        &self,
        configuration: &SessionConfiguration,
    ) -> ConstraintResult<()> {
        self.services
            .inference_profiles
            .resolve_provider(
                configuration.step_settings.collaboration_mode.model(),
                &configuration.original_config_do_not_use.model_provider_id,
                configuration.provider.info(),
            )
            .map(|_| ())
            .map_err(|error| ConstraintError::InvalidInferenceProfile {
                message: error.to_string(),
            })
    }

    // Every path that commits a session configuration resolves its profile first, so a turn
    // built from that configuration cannot be the first operation that starts its adapter.
    #[expect(clippy::expect_used)]
    pub(super) fn configure_turn_provider(
        &self,
        model: &str,
        configuration: &SessionConfiguration,
        per_turn_config: &mut Config,
    ) -> SharedModelProvider {
        let resolved_provider = self
            .services
            .inference_profiles
            .resolve_provider(
                model,
                &configuration.original_config_do_not_use.model_provider_id,
                configuration.provider.info(),
            )
            .expect("inference profile must be validated before building a turn context");
        per_turn_config.model_provider_id = resolved_provider.id;
        per_turn_config.model_provider = resolved_provider.info.clone();
        match resolved_provider.profile {
            Some(profile) => create_profile_model_provider(
                resolved_provider.info,
                Some(Arc::clone(&self.services.auth_manager)),
                Some(profile),
            ),
            None => Arc::clone(&configuration.provider),
        }
    }

    pub(crate) async fn turn_context_with_model(
        &self,
        turn_context: &TurnContext,
        model: String,
    ) -> Result<TurnContext, codex_inference_profiles::InferenceProfileError> {
        let (baseline_provider_id, baseline_provider) = {
            let state = self.state.lock().await;
            let configuration = &state.session_configuration;
            (
                configuration
                    .original_config_do_not_use
                    .model_provider_id
                    .clone(),
                configuration.provider.clone(),
            )
        };
        let resolved_provider = self.services.inference_profiles.resolve_provider(
            &model,
            &baseline_provider_id,
            baseline_provider.info(),
        )?;
        let provider = match resolved_provider.profile {
            Some(profile) => create_profile_model_provider(
                resolved_provider.info,
                turn_context.auth_manager.clone(),
                Some(profile),
            ),
            None => baseline_provider,
        };
        Ok(turn_context
            .with_model_and_provider(
                model,
                &self.services.models_manager,
                resolved_provider.id,
                provider,
            )
            .await)
    }
}
