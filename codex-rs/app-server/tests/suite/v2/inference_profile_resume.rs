//! Local profile restoration and managed-policy admission through public JSON-RPC.
//! Uses Unix child HOME isolation; portable profile behavior also has explicit-path core coverage.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_fake_rollout;
use app_test_support::rollout_path;
use codex_app_server_protocol::ModelProviderError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_inference_profiles::KIMI_K3_MODEL_ID;
use codex_inference_profiles::KIMI_PROVIDER_ID;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsAppliedEvent;
use codex_rollout::RolloutItem;
use codex_rollout::append_rollout_item_to_path;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::Duration;
use tokio::time::timeout;
use wiremock::MockServer;

#[tokio::test]
async fn cold_kimi_profile_resume_recreates_its_adapter_without_static_provider() -> Result<()> {
    let home = TempDir::new()?;
    let gateway = MockServer::start().await;
    MockResponsesConfig::new(&gateway.uri())
        .disable_feature(codex_features::Feature::ShellSnapshot)
        .write(home.path())?;
    std::fs::write(
        home.path().join(".kimi-codex"),
        "API_KEY=isolated-test-key\n",
    )?;
    let child_home = home.path().to_str().expect("test home path");
    let stamp = "2026-10-30T12-00-00";
    let thread_id = create_fake_rollout(
        home.path(),
        stamp,
        "2026-10-30T12:00:00Z",
        "Saved Kimi context",
        Some(KIMI_PROVIDER_ID),
        /*git_info*/ None,
    )?;
    let settings: ThreadSettingsAppliedEvent = serde_json::from_value(json!({
        "thread_id": thread_id,
        "thread_settings": {
            "model": KIMI_K3_MODEL_ID,
            "model_provider_id": KIMI_PROVIDER_ID,
            "cwd": home.path(),
            "approval_policy": "never",
            "approvals_reviewer": "user",
            "permission_profile": PermissionProfile::Disabled,
            "collaboration_mode": { "mode": "default", "settings": { "model": KIMI_K3_MODEL_ID } },
        },
    }))?;
    append_rollout_item_to_path(
        &rollout_path(home.path(), stamp, &thread_id),
        &RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(settings)),
    )
    .await?;
    for _ in 0..2 {
        let mut server = TestAppServer::builder()
            .with_codex_home(home.path())
            .with_env_overrides(&[("HOME", Some(child_home))])
            .build_initialized()
            .await?;
        let id = server
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: thread_id.clone(),
                exclude_turns: true,
                ..Default::default()
            })
            .await?;
        let resumed: ThreadResumeResponse =
            timeout(Duration::from_secs(/*secs*/ 30), server.read_response(id)).await??;
        assert_eq!(
            (resumed.thread.id, resumed.model, resumed.model_provider),
            (
                thread_id.clone(),
                KIMI_K3_MODEL_ID.to_owned(),
                KIMI_PROVIDER_ID.to_owned()
            )
        );
        assert!(server.shutdown_gracefully().await?.success());
    }
    assert!(
        gateway
            .received_requests()
            .await
            .expect("captured requests")
            .iter()
            .all(|request| !request.url.path().ends_with("/responses"))
    );
    Ok(())
}

#[tokio::test]
async fn changed_required_provider_rejects_profile_selection_before_missing_credential()
-> Result<()> {
    let home = TempDir::new()?;
    let gateway = MockServer::start().await;
    MockResponsesConfig::new(&gateway.uri())
        .disable_feature(codex_features::Feature::ShellSnapshot)
        .write(home.path())?;
    let child_home = home.path().to_str().expect("test home path");
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .with_env_overrides(&[("HOME", Some(child_home))])
        .build_initialized()
        .await?;
    let started = server.start_thread(ThreadStartParams::default()).await?;
    std::fs::write(
        home.path().join("requirements.toml"),
        "model_provider = 'mock_provider'\n",
    )?;
    let id = server
        .send_thread_settings_update_request(ThreadSettingsUpdateParams {
            thread_id: started.thread.id,
            model: Some(KIMI_K3_MODEL_ID.to_owned()),
            ..Default::default()
        })
        .await?;
    let error = timeout(
        Duration::from_secs(/*secs*/ 30),
        server.read_stream_until_error_message(RequestId::Integer(id)),
    )
    .await??;
    assert_eq!(
        error
            .error
            .data
            .map(serde_json::from_value::<ModelProviderError>)
            .transpose()?,
        Some(ModelProviderError::ModelProviderPolicyConflict {
            model_provider: KIMI_PROVIDER_ID.to_owned(),
            required_model_provider: "mock_provider".to_owned(),
        }),
    );
    assert!(server.shutdown_gracefully().await?.success());
    assert!(
        gateway
            .received_requests()
            .await
            .expect("captured requests")
            .iter()
            .all(|request| !request.url.path().ends_with("/responses"))
    );
    Ok(())
}
