use anyhow::Result;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_inference_profiles::InferenceProfileRuntime;
use codex_inference_profiles::KIMI_K3_MODEL_ID;
use codex_inference_profiles::KIMI_PROVIDER_ID;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadSettingsOverrides;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kimi_profile_resumes_from_persisted_effective_provider() -> Result<()> {
    let server = start_mock_server().await;
    let credentials = tempdir()?;
    let credential_file = credentials.path().join("kimi.env");
    std::fs::write(&credential_file, "API_KEY=test-key\n")?;
    let mut builder = test_codex()
        .with_model(KIMI_K3_MODEL_ID)
        .with_thread_extension(InferenceProfileRuntime::with_kimi_credential_file(
            &credential_file,
        ));
    let original = builder.build_with_auto_env(&server).await?;
    original.codex.ensure_rollout_materialized().await;
    let saved = original.codex.config_snapshot().await;

    // thread/resume feeds the saved effective model/provider back into configuration
    // loading. The derived provider must not require a static model_providers entry.
    let resumed_config = ConfigBuilder::default()
        .codex_home(original.home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            model: Some(saved.model),
            model_provider: Some(saved.model_provider_id),
            ..Default::default()
        })
        .build()
        .await?;
    let mut resume_builder = test_codex()
        .with_config(move |config| {
            config.model = resumed_config.model;
            config.model_provider_id = resumed_config.model_provider_id;
            config.model_provider = resumed_config.model_provider;
        })
        .with_thread_extension(InferenceProfileRuntime::with_kimi_credential_file(
            &credential_file,
        ));
    let resumed = resume_builder.restart(&server, &original).await?;
    assert_eq!(
        (
            resumed.session_configured.model.as_str(),
            resumed.session_configured.model_provider_id.as_str()
        ),
        (KIMI_K3_MODEL_ID, KIMI_PROVIDER_ID),
    );
    resumed.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kimi_profile_selects_embedded_provider_at_session_start() -> Result<()> {
    let server = start_mock_server().await;
    let credentials = tempdir()?;
    let credential_file = credentials.path().join("kimi.env");
    std::fs::write(&credential_file, "API_KEY=test-key\n")?;
    let runtime = InferenceProfileRuntime::with_kimi_credential_file(credential_file);
    let current_exe = std::env::current_exe()?;
    let mut builder = test_codex()
        .with_model(KIMI_K3_MODEL_ID)
        .with_config(move |config| config.codex_self_exe = Some(current_exe))
        .with_thread_extension(runtime);

    let test = builder.build_with_auto_env(&server).await?;
    let snapshot = test.codex.config_snapshot().await;

    assert_eq!(test.session_configured.model, KIMI_K3_MODEL_ID);
    assert_eq!(test.session_configured.model_provider_id, KIMI_PROVIDER_ID);
    assert_eq!(snapshot.model, KIMI_K3_MODEL_ID);
    assert_eq!(snapshot.model_provider_id, KIMI_PROVIDER_ID);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_kimi_credentials_reject_model_switch_without_changing_settings() -> Result<()> {
    let server = start_mock_server().await;
    let credentials = tempdir()?;
    let runtime =
        InferenceProfileRuntime::with_kimi_credential_file(credentials.path().join("missing.env"));
    let current_exe = std::env::current_exe()?;
    let mut builder = test_codex()
        .with_config(move |config| config.codex_self_exe = Some(current_exe))
        .with_thread_extension(runtime);
    let test = builder.build_with_auto_env(&server).await?;
    let initial_snapshot = test.codex.config_snapshot().await;

    let submission_id = test
        .codex
        .submit(Op::ThreadSettings {
            thread_settings: ThreadSettingsOverrides {
                model: Some(KIMI_K3_MODEL_ID.to_string()),
                ..Default::default()
            },
        })
        .await?;
    let error = loop {
        let event = test.codex.next_event().await?;
        if event.id == submission_id
            && let EventMsg::Error(error) = event.msg
        {
            break error;
        }
    };

    assert_eq!(
        error.message,
        "invalid thread settings override: Kimi K3 requires a Kimi Open Platform API key. Create ~/.kimi-codex with a non-empty API_KEY=<key> entry."
    );
    let current_snapshot = test.codex.config_snapshot().await;
    assert_eq!(
        (current_snapshot.model, current_snapshot.model_provider_id),
        (initial_snapshot.model, initial_snapshot.model_provider_id)
    );
    Ok(())
}
