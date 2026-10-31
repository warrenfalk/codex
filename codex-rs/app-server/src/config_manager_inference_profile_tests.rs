//! Admission of registered local profiles alongside ordinary provider enforcement.

use super::*;
use anyhow::Result;
use codex_config::test_support::CloudConfigBundleFixture;
use codex_inference_profiles::KIMI_K3_MODEL_ID;
use codex_inference_profiles::KIMI_PROVIDER_ID;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn cold_profile_resume_retains_baseline_without_resolving_credentials() -> Result<()> {
    let home = tempdir()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let config = manager
        .load_for_resume(
            Some(KIMI_PROVIDER_ID),
            /*request_overrides*/ None,
            ConfigOverrides {
                model: Some(KIMI_K3_MODEL_ID.to_owned()),
                model_provider: Some(KIMI_PROVIDER_ID.to_owned()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await?;
    assert_eq!(
        (config.model, config.model_provider_id),
        (Some(KIMI_K3_MODEL_ID.to_owned()), "openai".to_owned()),
    );
    Ok(())
}

#[tokio::test]
async fn profile_alias_does_not_bypass_unknown_saved_or_requested_provider() -> Result<()> {
    let home = tempdir()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    for (model, saved, requested, unavailable) in [
        (
            "other-model",
            KIMI_PROVIDER_ID,
            KIMI_PROVIDER_ID,
            KIMI_PROVIDER_ID,
        ),
        (KIMI_K3_MODEL_ID, "unknown", KIMI_PROVIDER_ID, "unknown"),
        (KIMI_K3_MODEL_ID, KIMI_PROVIDER_ID, "unknown", "unknown"),
    ] {
        let error = manager
            .load_for_resume(
                Some(saved),
                /*request_overrides*/ None,
                ConfigOverrides {
                    model: Some(model.to_owned()),
                    model_provider: Some(requested.to_owned()),
                    ..Default::default()
                },
                Some(home.path().to_path_buf()),
            )
            .await
            .expect_err("unregistered provider must remain rejected");
        assert_eq!(
            error
                .get_ref()
                .and_then(|error| error.downcast_ref::<ModelProviderError>()),
            Some(&ModelProviderError::ModelProviderNotConfigured {
                model_provider: unavailable.to_owned(),
            }),
        );
    }
    Ok(())
}

#[tokio::test]
async fn required_provider_rejects_profile_resume_and_live_model_before_credentials() -> Result<()>
{
    let home = tempdir()?;
    let initial = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let baseline = initial
        .load_latest_config(Some(home.path().to_path_buf()))
        .await?;
    let restricted = ConfigManager::new_for_tests(
        home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        CloudConfigBundleFixture::loader_with_enterprise_requirement("model_provider = 'openai'"),
    );
    let resume_error = restricted
        .load_for_resume(
            Some(KIMI_PROVIDER_ID),
            /*request_overrides*/ None,
            ConfigOverrides {
                model: Some(KIMI_K3_MODEL_ID.to_owned()),
                model_provider: Some(KIMI_PROVIDER_ID.to_owned()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await
        .expect_err("policy blocks the profile before materialization");
    let input_error = restricted
        .check_thread_model_provider_for_input(&baseline, KIMI_K3_MODEL_ID)
        .await
        .expect_err("current model must override baseline model for policy admission");
    for error in [resume_error, input_error] {
        assert_eq!(
            error
                .get_ref()
                .and_then(|error| error.downcast_ref::<ModelProviderError>()),
            Some(&ModelProviderError::ModelProviderPolicyConflict {
                model_provider: KIMI_PROVIDER_ID.to_owned(),
                required_model_provider: "openai".to_owned(),
            }),
        );
    }
    Ok(())
}

#[tokio::test]
async fn local_profile_cannot_replace_a_matching_managed_provider_definition() -> Result<()> {
    let home = tempdir()?;
    let definition =
        "[model_providers.kimi]\nname = 'Managed route'\nbase_url = 'http://127.0.0.1:1/v1'\n";
    std::fs::write(
        home.path().join("config.toml"),
        format!("model = 'other-model'\nmodel_provider = 'kimi'\n{definition}"),
    )?;
    let initial = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let baseline = initial
        .load_latest_config(Some(home.path().to_path_buf()))
        .await?;
    let restricted = ConfigManager::new_for_tests(
        home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        CloudConfigBundleFixture::loader_with_enterprise_requirement(definition),
    );
    let error = restricted
        .check_thread_model_provider_for_input(&baseline, KIMI_K3_MODEL_ID)
        .await
        .expect_err("local adapter must not replace the managed route");
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(&ModelProviderError::ModelProviderPolicyConflict {
            model_provider: KIMI_PROVIDER_ID.to_owned(),
            required_model_provider: KIMI_PROVIDER_ID.to_owned(),
        }),
    );
    Ok(())
}
