use super::model_with_approval_messages;
use super::permissions_texts;
use anyhow::Context;
use anyhow::Result;
use codex_config::LoaderOverrides;
use codex_core::CodexThread;
use codex_core::RolloutRecorder;
use codex_core::TurnInputRequest;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_login::CodexAuth;
use codex_protocol::models::ActivePermissionProfile;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutItem;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::submit_thread_settings;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::MockServer;

async fn sample_permissions(thread: &CodexThread, server: &MockServer) -> Result<Vec<String>> {
    let response = mount_sse_once(
        server,
        sse(vec![ev_response_created("resp"), ev_completed("resp")]),
    )
    .await;
    thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(thread, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    Ok(permissions_texts(&response.single_request()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_instructions_follow_inheritance_model_changes_switches_and_resume() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let home = Arc::new(TempDir::new()?);
    std::fs::write(home.path().join("parent.md"), "Parent policy instructions")?;
    std::fs::write(home.path().join("child.md"), "Child policy instructions")?;
    std::fs::write(home.path().join("empty.md"), "")?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"approval_policy = "trust-sandbox"
default_permissions = "inherited"
[permissions.base]
extends = ":workspace"
instructions_file = "parent.md"
[permissions.inherited]
extends = "base"
[permissions.child]
extends = "base"
instructions_file = "child.md"
[permissions.empty]
extends = "child"
instructions_file = "empty.md"
"#,
    )?;
    let server = start_mock_server().await;
    let mut builder = test_codex()
        .with_home(home.clone())
        .with_model("model-a")
        .with_config(|config| {
            config.model_catalog = Some(ModelsResponse {
                models: vec![
                    model_with_approval_messages("model-a", "Catalog A", "Catalog A auto-review"),
                    model_with_approval_messages("model-b", "Catalog B", "Catalog B auto-review"),
                ],
            });
        });
    let test = builder.build_with_auto_env(&server).await?;
    let first = sample_permissions(&test.codex, &server).await?;
    assert_eq!(first.len(), 1);
    assert!(first[0].contains("Parent policy instructions"));
    assert!(first[0].contains("The writable"));
    assert!(!first[0].contains("Trust Sandbox Behavior"));

    submit_thread_settings(
        &test.codex,
        ThreadSettingsOverrides {
            model: Some("model-b".to_string()),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(sample_permissions(&test.codex, &server).await?, first);

    submit_thread_settings(
        &test.codex,
        ThreadSettingsOverrides {
            permission_profile: Some(test.config.permissions.permission_profile().clone()),
            active_permission_profile: Some(ActivePermissionProfile {
                id: "child".to_string(),
                extends: Some("base".to_string()),
            }),
            ..Default::default()
        },
    )
    .await?;
    let changed = sample_permissions(&test.codex, &server).await?;
    assert_eq!(changed.len(), 2);
    assert_eq!(changed[0], first[0]);
    assert!(changed[1].contains("Child policy instructions"));
    assert!(!changed[1].contains("Parent policy instructions"));

    test.codex.shutdown_and_wait().await?;
    let history = RolloutRecorder::get_rollout_history(
        &test
            .session_configured
            .rollout_path
            .clone()
            .expect("rollout"),
    )
    .await?;
    let persisted = history
        .get_rollout_items()
        .iter()
        .rev()
        .find_map(|item| match item {
            RolloutItem::TurnContext(context) => Some(context),
            _ => None,
        })
        .context("persisted turn settings")?;
    // App-server applies the persisted selection while loading resume configuration;
    // core's resume API intentionally uses the configuration supplied by its caller.
    let loaded = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .harness_overrides(ConfigOverrides {
            cwd: Some(test.config.cwd.to_path_buf()),
            persisted_permission_profile_id: persisted
                .active_permission_profile
                .as_ref()
                .map(|profile| profile.id.clone()),
            approval_policy: Some(persisted.approval_policy),
            approvals_reviewer: persisted.approvals_reviewer,
            ..Default::default()
        })
        .build()
        .await?;
    let mut resume_config = test.config.clone();
    resume_config.permissions = loaded.permissions;
    resume_config.model = Some(persisted.model.clone());
    let auth_manager = codex_core::test_support::auth_manager_from_auth_with_home(
        CodexAuth::from_api_key("test-key"),
        home.path().to_path_buf(),
    );
    let resumed = test
        .thread_manager
        .resume_thread_with_history(
            resume_config,
            history,
            auth_manager,
            /*parent_trace*/ None,
            Default::default(),
        )
        .await?;
    let after_resume = sample_permissions(&resumed.thread, &server).await?;
    assert!(
        after_resume
            .last()
            .unwrap()
            .contains("Child policy instructions"),
        "{after_resume:?}"
    );

    submit_thread_settings(
        &resumed.thread,
        ThreadSettingsOverrides {
            permission_profile: Some(test.config.permissions.permission_profile().clone()),
            active_permission_profile: Some(ActivePermissionProfile {
                id: "empty".to_string(),
                extends: Some("child".to_string()),
            }),
            ..Default::default()
        },
    )
    .await?;
    let empty = sample_permissions(&resumed.thread, &server).await?;
    let latest = empty.last().unwrap();
    assert!(latest.contains("## Active permissions"));
    assert!(!latest.contains("Child policy instructions"));
    assert!(!latest.contains("Trust Sandbox Behavior"));

    submit_thread_settings(
        &resumed.thread,
        ThreadSettingsOverrides {
            permission_profile: Some(PermissionProfile::read_only()),
            approval_policy: Some(AskForApproval::OnRequest),
            ..Default::default()
        },
    )
    .await?;
    let fallback = sample_permissions(&resumed.thread, &server).await?;
    assert!(fallback.last().unwrap().contains("Catalog B"));
    Ok(())
}
