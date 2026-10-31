use super::*;
use codex_app_server_protocol::ClientInfo;
use codex_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn embedded_startup_delivers_config_warning_without_thread_subscriptions() {
    let home = tempfile::tempdir().unwrap();
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await
        .unwrap();
    let expected = ConfigWarningNotification {
        summary: "startup warning".into(),
        details: None,
        path: None,
        range: None,
    };
    let mut client = start(InProcessStartArgs {
        arg0_paths: Arg0DispatchPaths::default(),
        config: Arc::new(config),
        cli_overrides: Vec::new(),
        loader_overrides: LoaderOverrides::without_managed_config_for_tests(),
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        embedded_network_policy: EmbeddedNetworkPolicy::default(),
        thread_config_loader: Arc::new(codex_config::NoopThreadConfigLoader),
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
        config_warnings: vec![expected.clone()],
        session_source: SessionSource::Cli,
        enable_codex_api_key_env: false,
        initialize: InitializeParams {
            client_info: ClientInfo {
                name: "startup-warning-test".into(),
                title: None,
                version: "0.0.0".into(),
            },
            capabilities: None,
        },
        channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await
    .unwrap();

    let warning = timeout(Duration::from_secs(/*secs*/ 5), async {
        loop {
            if let InProcessServerEvent::ServerNotification(notification) =
                client.next_event().await.unwrap()
                && let ServerNotification::ConfigWarning(warning) = *notification
            {
                break warning;
            }
        }
    })
    .await
    .expect("embedded initialization should deliver its configuration warnings");
    assert_eq!(warning, expected);
    client.shutdown().await.unwrap();
}
