use super::*;
use pretty_assertions::assert_eq;

#[test]
fn non_profile_model_preserves_the_baseline_provider() -> anyhow::Result<()> {
    let runtime = InferenceProfileRuntime::default();
    let baseline = ModelProviderInfo {
        name: "baseline".to_string(),
        base_url: Some("https://example.test/v1".to_string()),
        ..ModelProviderInfo::default()
    };

    let resolved = runtime.resolve_provider("other-model", "custom", &baseline)?;

    assert_eq!(resolved.id, "custom");
    assert_eq!(resolved.info, baseline);
    assert_eq!(resolved.profile, None);
    Ok(())
}

#[tokio::test]
async fn kimi_profile_starts_one_loopback_adapter() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_path = temp.path().join("kimi.env");
    std::fs::write(&credential_path, "API_KEY='test-key'\n")?;
    let runtime = InferenceProfileRuntime::with_kimi_credential_file(credential_path);

    let first =
        runtime.resolve_provider(KIMI_K3_MODEL_ID, "openai", &ModelProviderInfo::default())?;
    let second =
        runtime.resolve_provider(KIMI_K3_MODEL_ID, "openai", &ModelProviderInfo::default())?;

    assert_eq!(first.id, KIMI_PROVIDER_ID);
    assert_eq!(first.profile, Some(InferenceProfile::KimiK3));
    assert_eq!(first.info.base_url, second.info.base_url);
    assert!(
        first
            .info
            .base_url
            .as_deref()
            .is_some_and(|url| url.starts_with("http://127.0.0.1:"))
    );
    Ok(())
}

#[test]
fn kimi_profile_reports_actionable_missing_credentials() {
    let runtime = InferenceProfileRuntime::with_kimi_credential_file(PathBuf::from(
        "/definitely/missing/.kimi-codex",
    ));

    let error = runtime
        .resolve_provider(KIMI_K3_MODEL_ID, "openai", &ModelProviderInfo::default())
        .err()
        .map(|error| error.to_string());

    assert_eq!(
        error.as_deref(),
        Some(
            "Kimi K3 requires a Kimi Open Platform API key. Create ~/.kimi-codex with a non-empty API_KEY=<key> entry."
        )
    );
}
