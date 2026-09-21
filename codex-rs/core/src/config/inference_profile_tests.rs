use super::Config;
use super::ConfigOverrides;
use codex_config::config_toml::ConfigToml;
use codex_inference_profiles::InferenceProfileRuntime;
use codex_inference_profiles::KIMI_K3_MODEL_ID;
use codex_inference_profiles::KIMI_PROVIDER_ID;
use core_test_support::TempDirExt;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn persisted_profile_provider_restores_configured_baseline() -> anyhow::Result<()> {
    let home = tempdir()?;
    let credential_path = home.path().join("kimi.env");
    std::fs::write(&credential_path, "API_KEY=test-key\n")?;
    for (base, expected_provider) in [
        ("", "openai"),
        ("model_provider = 'kimi'", "openai"),
        (
            "model_provider = 'custom'\n[model_providers.custom]\nname = 'Custom'\nbase_url = 'http://127.0.0.1:1/v1'",
            "custom",
        ),
    ] {
        let config = Config::load_from_base_config_with_overrides(
            toml::from_str(base)?,
            ConfigOverrides {
                model: Some(KIMI_K3_MODEL_ID.to_string()),
                model_provider: Some(KIMI_PROVIDER_ID.to_string()),
                ..Default::default()
            },
            home.abs(),
        )
        .await?;
        assert_eq!(
            (&config.model, config.model_provider_id.as_str()),
            (&Some(KIMI_K3_MODEL_ID.to_string()), expected_provider)
        );

        // The alias never becomes a static endpoint. Startup resolves it afresh, and
        // selecting another model still restores the actual configured baseline.
        let runtime = InferenceProfileRuntime::with_kimi_credential_file(&credential_path);
        let kimi = runtime.resolve_provider(
            KIMI_K3_MODEL_ID,
            &config.model_provider_id,
            &config.model_provider,
        )?;
        assert_eq!(kimi.id, KIMI_PROVIDER_ID);
        assert!(
            kimi.info
                .base_url
                .as_deref()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        let baseline = runtime.resolve_provider(
            "other-model",
            &config.model_provider_id,
            &config.model_provider,
        )?;
        assert_eq!(
            (baseline.id, baseline.info),
            (expected_provider.to_string(), config.model_provider)
        );
    }
    Ok(())
}

#[tokio::test]
async fn profile_alias_in_base_config_uses_the_effective_model() -> anyhow::Result<()> {
    let home = tempdir()?;
    let config = Config::load_from_base_config_with_overrides(
        toml::from_str("model = 'kimi-k3'\nmodel_provider = 'kimi'")?,
        ConfigOverrides::default(),
        home.abs(),
    )
    .await?;
    assert_eq!(
        (config.model, config.model_provider_id),
        (Some(KIMI_K3_MODEL_ID.to_string()), "openai".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn unrelated_unknown_providers_still_fail_configuration() -> anyhow::Result<()> {
    let home = tempdir()?;
    for (model, provider) in [
        ("other-model", KIMI_PROVIDER_ID),
        (KIMI_K3_MODEL_ID, "typo"),
    ] {
        let error = Config::load_from_base_config_with_overrides(
            ConfigToml::default(),
            ConfigOverrides {
                model: Some(model.to_string()),
                model_provider: Some(provider.to_string()),
                ..Default::default()
            },
            home.abs(),
        )
        .await
        .expect_err("only a matching built-in profile may bypass configured provider lookup");
        assert_eq!(
            (error.kind(), error.to_string()),
            (
                std::io::ErrorKind::NotFound,
                format!("Model provider `{provider}` not found")
            )
        );
    }
    Ok(())
}
