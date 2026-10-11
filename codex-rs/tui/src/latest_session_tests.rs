use super::*;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use crate::latest_session_cwd_filter;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use crate::lookup_latest_session_target_with_app_server;
use crate::tests::start_test_embedded_app_server;
use crate::tests::write_session_rollout;
use codex_app_server_protocol::ThreadSourceKind;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test]
fn latest_session_lookup_params_keep_local_filters_for_embedded_sessions() -> std::io::Result<()> {
    let temp_dir = TempDir::new()?;
    let cwd = temp_dir.path().join("project");

    let params = latest_session_lookup_params(
        /*uses_remote_filesystem*/ false,
        /*model_provider*/ None,
        LastSessionScope::Cwd,
        Some(cwd.as_path()),
        /*include_non_interactive*/ false,
        LatestSessionLookupMode::StateDbOnly,
    );

    assert_eq!(params.model_providers, None);
    assert_eq!(
        params.cwd,
        Some(ThreadListCwdFilter::One(cwd.to_string_lossy().to_string()))
    );
    assert!(params.use_state_db_only);

    let scan_params = latest_session_lookup_params(
        /*uses_remote_filesystem*/ false,
        /*model_provider*/ None,
        LastSessionScope::Cwd,
        Some(cwd.as_path()),
        /*include_non_interactive*/ false,
        LatestSessionLookupMode::ScanAndRepair,
    );
    assert!(!scan_params.use_state_db_only);
    Ok(())
}

#[test]
fn latest_session_lookup_params_honor_explicit_provider() -> color_eyre::Result<()> {
    let temp_dir = TempDir::new()?;
    let cwd = temp_dir.path().join("project");
    let params = latest_session_lookup_params(
        /*uses_remote_filesystem*/ false,
        Some("selected-provider".to_string()),
        LastSessionScope::Cwd,
        Some(cwd.as_path()),
        /*include_non_interactive*/ false,
        LatestSessionLookupMode::StateDbOnly,
    );

    assert_eq!(
        params.model_providers,
        Some(vec!["selected-provider".to_string()])
    );
    assert_eq!(
        params.cwd,
        Some(ThreadListCwdFilter::One(cwd.to_string_lossy().to_string()))
    );
    Ok(())
}

#[test]
fn latest_session_lookup_params_omit_local_filters_for_remote_sessions() -> std::io::Result<()> {
    let params = latest_session_lookup_params(
        /*uses_remote_filesystem*/ true,
        /*model_provider*/ None,
        LastSessionScope::Cwd,
        /*cwd_filter*/ None,
        /*include_non_interactive*/ false,
        LatestSessionLookupMode::StateDbOnly,
    );

    assert_eq!(params.model_providers, None);
    assert_eq!(params.cwd, None);
    Ok(())
}

#[test]
fn latest_session_lookup_params_can_include_non_interactive_sources() -> std::io::Result<()> {
    let params = latest_session_lookup_params(
        /*uses_remote_filesystem*/ true,
        /*model_provider*/ None,
        LastSessionScope::Cwd,
        /*cwd_filter*/ None,
        /*include_non_interactive*/ true,
        LatestSessionLookupMode::StateDbOnly,
    );

    assert_eq!(
        params.source_kinds,
        Some(vec![
            ThreadSourceKind::Cli,
            ThreadSourceKind::VsCode,
            ThreadSourceKind::Exec,
            ThreadSourceKind::AppServer,
        ])
    );
    Ok(())
}

#[test]
fn latest_session_lookup_params_keep_explicit_cwd_filter_for_remote_sessions() -> std::io::Result<()>
{
    let cwd = Path::new("repo/on/server");

    let params = latest_session_lookup_params(
        /*uses_remote_filesystem*/ true,
        /*model_provider*/ None,
        LastSessionScope::Cwd,
        Some(cwd),
        /*include_non_interactive*/ false,
        LatestSessionLookupMode::StateDbOnly,
    );

    assert_eq!(params.model_providers, None);
    assert_eq!(
        params.cwd,
        Some(ThreadListCwdFilter::One(String::from("repo/on/server")))
    );
    Ok(())
}

#[tokio::test]
async fn latest_session_scope_separates_cwd_repo_and_all() -> color_eyre::Result<()> {
    let temp_dir = TempDir::new()?;
    let project_cwd = temp_dir.path().join("project");
    let linked_cwd = temp_dir.path().join("linked-project");
    let other_cwd = temp_dir.path().join("other-project");
    let admin = project_cwd.join(".git/worktrees/linked");
    std::fs::create_dir_all(&admin)?;
    std::fs::create_dir_all(&linked_cwd)?;
    std::fs::create_dir_all(&other_cwd)?;
    std::fs::write(project_cwd.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    std::fs::write(admin.join("commondir"), "../..\n")?;
    std::fs::write(
        admin.join("gitdir"),
        linked_cwd.join(".git").display().to_string(),
    )?;
    std::fs::write(
        linked_cwd.join(".git"),
        format!("gitdir: {}", admin.display()),
    )?;

    let mut config = ConfigBuilder::default()
        .codex_home(temp_dir.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(project_cwd.clone()),
            ..Default::default()
        })
        .build()
        .await?;
    config
        .features
        .set_enabled(codex_features::Feature::Worktrees, /*enabled*/ false)?;
    let model_provider = config.model_provider_id.as_str();
    let project_thread_id = write_session_rollout(
        temp_dir.path(),
        "2025-01-02T10-00-00",
        "2025-01-02T10:00:00Z",
        "older project session",
        model_provider,
        &project_cwd,
    )?;
    let linked_thread_id = write_session_rollout(
        temp_dir.path(),
        "2025-01-02T11-00-00",
        "2025-01-02T11:00:00Z",
        "newer linked-worktree session",
        model_provider,
        &linked_cwd,
    )?;
    let other_thread_id = write_session_rollout(
        temp_dir.path(),
        "2025-01-02T12-00-00",
        "2025-01-02T12:00:00Z",
        "newer other project session",
        model_provider,
        &other_cwd,
    )?;

    let mut app_server = AppServerSession::new(
        codex_app_server_client::AppServerClient::InProcess(
            start_test_embedded_app_server(config.clone()).await?,
        ),
        ThreadParamsMode::Embedded,
    );
    // An explicit scope wins over the feature flag in both directions.
    for worktrees_enabled in [false, true] {
        config
            .features
            .set_enabled(codex_features::Feature::Worktrees, worktrees_enabled)?;
        for (scope, show_all, expected_id) in [
            (LastSessionScope::Cwd, false, project_thread_id),
            (LastSessionScope::Repo, false, linked_thread_id),
            (LastSessionScope::Cwd, true, other_thread_id),
        ] {
            let filter_cwd = latest_session_cwd_filter(
                /*uses_remote_workspace*/ false, /*remote_cwd_override*/ None, &config,
                show_all,
            );
            let target = lookup_latest_session_target_with_app_server(
                /*uses_remote_filesystem*/ false,
                &mut app_server,
                &config,
                filter_cwd,
                scope,
                /*include_non_interactive*/ false,
            )
            .await?
            .expect("expected a session in the selected scope");
            assert_eq!(target.thread_id, expected_id);
        }
    }

    let empty_cwd = project_cwd.join("empty");
    std::fs::create_dir(&empty_cwd)?;
    assert_eq!(
        lookup_latest_session_target_with_app_server(
            /*uses_remote_filesystem*/ false,
            &mut app_server,
            &config,
            Some(&empty_cwd),
            LastSessionScope::Cwd,
            /*include_non_interactive*/ false,
        )
        .await?
        .map(|target| target.thread_id),
        None,
    );
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn latest_session_lookup_falls_back_for_rollout_missing_from_state_db()
-> color_eyre::Result<()> {
    let temp_dir = TempDir::new()?;
    let project_cwd = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_cwd)?;
    let config = ConfigBuilder::default()
        .codex_home(temp_dir.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(project_cwd.clone()),
            ..Default::default()
        })
        .build()
        .await?;
    let mut app_server = AppServerSession::new(
        codex_app_server_client::AppServerClient::InProcess(
            start_test_embedded_app_server(config.clone()).await?,
        ),
        ThreadParamsMode::Embedded,
    );

    // Simulate a legacy writer creating a rollout after the state DB backfill completed.
    let thread_id = write_session_rollout(
        temp_dir.path(),
        "2025-01-02T10-00-00",
        "2025-01-02T10:00:00Z",
        "legacy writer session",
        config.model_provider_id.as_str(),
        &project_cwd,
    )?;

    let target = lookup_latest_session_target_with_app_server(
        /*uses_remote_filesystem*/ false,
        &mut app_server,
        &config,
        Some(project_cwd.as_path()),
        LastSessionScope::Cwd,
        /*include_non_interactive*/ false,
    )
    .await?
    .expect("expected scan-and-repair fallback to find the rollout");
    app_server.shutdown().await?;

    assert_eq!(target.thread_id, thread_id);
    Ok(())
}
