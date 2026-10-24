//! Exercise project attachment through the real TUI dispatch and app-server boundary.

use super::*;
use crate::app_command::AppCommand;
use crate::app_server_session::ResumeModelSettings;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn project_attachment_preserves_roots_across_later_turns_resume_and_fork() -> Result<()> {
    let model_server = responses::start_mock_server().await;
    let home = tempdir()?;
    let workspace = tempdir()?;
    let other = tempdir()?;
    let root = workspace.path().canonicalize()?.abs();
    let other_root = other.path().canonicalize()?.abs();
    let url = model_server.uri();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model = "gpt-5.2"
model_provider = "project-test"
sandbox_mode = "workspace-write"
[model_providers.project-test]
name = "OpenAI"
base_url = "{url}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )?;
    let mut app = Box::pin(make_test_app()).await;
    app.config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(root.to_path_buf()),
            ..Default::default()
        })
        .build()
        .await?;
    app.config.auto_thread_title = false;
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let state_db =
        crate::init_state_db_for_app_server_target(&app.config, &crate::AppServerTarget::Embedded)
            .await?
            .expect("state database");
    let mut other_config = app.config.clone();
    other_config.cwd = other_root.clone();
    for _ in 0..2 {
        let sibling = server.start_thread(&other_config).await?;
        server
            .thread_inject_items(
                sibling.session.thread_id,
                vec![responses::user_message_item("Other project conversation")],
            )
            .await?;
        state_db
            .set_thread_preview_if_empty(sibling.session.thread_id, "Other project conversation")
            .await
            .expect("seed sibling preview");
        server
            .thread_set_name(sibling.session.thread_id, "Other project".to_string())
            .await?;
    }
    let started = server.start_thread(&app.config).await?;
    let thread_id = started.session.thread_id;
    server
        .thread_inject_items(
            thread_id,
            vec![responses::user_message_item("Main project conversation")],
        )
        .await?;
    state_db
        .set_thread_preview_if_empty(thread_id, "Main project conversation")
        .await
        .expect("seed main preview");
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    let initial_roots = app
        .chat_widget
        .config_ref()
        .permissions
        .user_visible_workspace_roots()
        .to_vec();
    let initial_profile = app
        .chat_widget
        .config_ref()
        .permissions
        .permission_profile()
        .clone();
    let mut expected_roots = initial_roots.clone();
    expected_roots.push(other_root.clone());
    let (recent_tx, mut recent_rx) = tokio::sync::mpsc::unbounded_channel();
    let _recent_search = crate::project_mentions::spawn_search(
        server.request_handle(),
        thread_id,
        String::new(),
        root.clone(),
        AppEventSender::new(recent_tx),
    );
    let recent = tokio::time::timeout(Duration::from_secs(/*secs*/ 15), recent_rx.recv())
        .await?
        .unwrap();
    let AppEvent::ProjectSearchResult { result, .. } = recent else {
        panic!("expected recent projects");
    };
    let mut recent_roots = result.unwrap();
    recent_roots.sort();
    let mut distinct_roots = vec![root.clone(), other_root.clone()];
    distinct_roots.sort();
    assert_eq!(recent_roots, distinct_roots);
    let file = root.join("file.txt");
    std::fs::write(file.as_path(), "not a directory")?;
    for invalid in [file, root.join("missing")] {
        assert!(
            crate::project_mentions::validate_projects(&server.request_handle(), &[invalid])
                .await
                .is_err()
        );
    }
    // Discover completion and validate on the server, never against an unrelated client cwd.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _search = crate::project_mentions::spawn_search(
        server.request_handle(),
        thread_id,
        other_root.to_string_lossy().into_owned(),
        root.clone(),
        AppEventSender::new(tx),
    );
    let event = tokio::time::timeout(Duration::from_secs(/*secs*/ 15), rx.recv())
        .await?
        .unwrap();
    let AppEvent::ProjectSearchResult { result, .. } = event else {
        panic!("expected project results");
    };
    assert_eq!(result.unwrap(), vec![other_root.clone()]);

    let model_response = responses::mount_sse_sequence(
        &model_server,
        (0..2)
            .map(|index| {
                let id = format!("response-{index}");
                responses::sse(vec![
                    responses::ev_response_created(&id),
                    responses::ev_completed(&id),
                ])
            })
            .collect(),
    )
    .await;
    for index in 0..2 {
        let mut items = vec![UserInput::Text {
            text: "Work on both projects".to_string(),
            text_elements: Vec::new(),
        }];
        if index == 0 {
            items.push(UserInput::Mention {
                name: "project-other".to_string(),
                path: crate::project_mentions::project_target(&other_root.to_string_lossy()),
            });
        }
        let config = app.chat_widget.config_ref();
        let op = AppCommand::user_turn(
            format!("project-message-{index}"),
            items,
            root.to_path_buf(),
            config.permissions.approval_policy.value().into(),
            config.permissions.active_permission_profile(),
            "gpt-5.2".to_string(),
            /*effort*/ None,
            /*summary*/ None,
            /*service_tier*/ None,
            /*final_output_json_schema*/ None,
            /*collaboration_mode*/ None,
        );
        assert!(
            app.try_submit_active_thread_op_via_app_server(&mut server, thread_id, &op)
                .await?
        );
        tokio::time::timeout(Duration::from_secs(/*secs*/ 30), async {
            while let Some(event) = server.next_event().await {
                if event.backend_id == server.backend_id()
                    && let AppServerEvent::ServerNotification(notification) = event.event
                    && let ServerNotification::TurnCompleted(completed) = *notification
                {
                    assert_eq!(
                        completed.turn.status,
                        codex_app_server_protocol::TurnStatus::Completed
                    );
                    return;
                }
            }
            panic!("server disconnected");
        })
        .await?;
        assert_eq!(
            app.chat_widget
                .config_ref()
                .permissions
                .user_visible_workspace_roots(),
            expected_roots
        );
        assert_eq!(
            app.chat_widget
                .config_ref()
                .permissions
                .permission_profile(),
            &initial_profile
        );
        let profile = app
            .chat_widget
            .config_ref()
            .permissions
            .effective_permission_profile();
        let filesystem = profile.file_system_sandbox_policy();
        assert!(filesystem.can_write_local_path_with_cwd(other_root.as_path(), root.as_path()));
        assert_eq!(
            app.primary_session_configured
                .as_ref()
                .unwrap()
                .runtime_workspace_roots,
            expected_roots
        );
    }
    assert_eq!(model_response.requests().len(), 2);
    let resumed = server
        .resume_thread(
            &app.local_settings,
            app.config.clone(),
            thread_id,
            ResumeModelSettings::RestoreFromThread,
        )
        .await?;
    assert_eq!(resumed.session.runtime_workspace_roots, expected_roots);
    let forked = server
        .fork_thread(
            &app.local_settings,
            app.chat_widget.config_ref().clone(),
            thread_id,
        )
        .await?;
    assert_eq!(forked.session.runtime_workspace_roots, expected_roots);
    server.shutdown().await?;
    Ok(())
}
