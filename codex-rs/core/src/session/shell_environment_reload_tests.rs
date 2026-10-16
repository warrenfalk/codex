use super::*;
use crate::config::ConfigBuilder;
use crate::session::tests::make_session_and_context;
use crate::session::tests::make_session_and_context_with_rx;
use codex_protocol::config_types::ShellEnvironmentPolicy;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn captured_environment_keeps_its_policy_after_next_turn_reload() {
    let (session, _) = make_session_and_context().await;
    let home = tempfile::tempdir().unwrap();
    let initial = ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_path_buf())
        .build()
        .await
        .unwrap();
    session
        .state
        .lock()
        .await
        .session_configuration
        .original_config_do_not_use = Arc::new(initial);
    let mut captured = Vec::new();
    for value in ["first", "second"] {
        std::fs::write(
            home.path().join("config.toml"),
            format!("[shell_environment_policy.set]\nCODEX_TEST_RELOADED_ENV = '{value}'"),
        )
        .unwrap();
        let mut configuration = session.state.lock().await.session_configuration.clone();
        session
            .reload_shell_environment(&mut configuration, value)
            .await;
        captured.push(session.activate_turn_environments(&configuration).await);
    }
    let policies = captured
        .iter()
        .map(|snapshot| {
            snapshot
                .primary()
                .unwrap()
                .shell_environment_policy()
                .clone()
        })
        .collect::<Vec<_>>();
    let expected = ["first", "second"].map(|value| {
        let mut policy = ShellEnvironmentPolicy::default();
        policy
            .r#set
            .insert("CODEX_TEST_RELOADED_ENV".to_string(), value.to_string());
        policy
    });
    assert_eq!(policies, expected);
}

#[tokio::test]
async fn failed_reload_keeps_previous_policy_and_warns_without_config_contents() {
    let (session, _, events) = make_session_and_context_with_rx().await;
    let home = tempfile::tempdir().unwrap();
    let initial = ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_path_buf())
        .build()
        .await
        .unwrap();
    session
        .state
        .lock()
        .await
        .session_configuration
        .original_config_do_not_use = Arc::new(initial);
    let mut configuration = session.state.lock().await.session_configuration.clone();
    let previous = configuration.shell_environment_policy.clone();
    std::fs::write(
        home.path().join("config.toml"),
        "[shell_environment_policy.set]\nGH_TOKEN = synthetic-secret-invalid-toml",
    )
    .unwrap();
    session
        .reload_shell_environment(&mut configuration, "failed-turn")
        .await;
    assert_eq!(configuration.shell_environment_policy, previous);
    assert_eq!(
        serde_json::to_value(events.recv().await.unwrap()).unwrap(),
        serde_json::to_value(Event {
            id: "failed-turn".to_string(),
            msg: EventMsg::Warning(WarningEvent {
                message: "Could not reload shell environment configuration; keeping the previous environment policy for this turn.".to_string(),
            }),
        }).unwrap(),
    );
}
