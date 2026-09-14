use super::*;
use crate::agents_focus::Endpoint;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn focus_matches_live_primary_session_after_switching_and_ignores_retained_sessions()
-> Result<()> {
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let endpoint = crate::resolve_remote_addr("ws://localhost:5000")?;
    let first = ThreadId::new();
    let second = ThreadId::new();
    app.app_server_target = AppServerTarget::Remote {
        endpoint: endpoint.clone(),
    };
    let key = Endpoint::from(&endpoint);
    for (current, expected) in [(first, [true, false]), (second, [false, true])] {
        app.enqueue_primary_thread_session(
            test_thread_session(current, app.config.cwd.to_path_buf()),
            Vec::new(),
        )
        .await?;
        assert_eq!(
            [
                app.matches_focus_session(&key, &first.to_string()),
                app.matches_focus_session(&key, &second.to_string())
            ],
            expected
        );
    }
    assert!(app.agents_overview.model.threads.contains_key(&first));
    // Viewing a subagent keeps the primary family attached.
    let child = ThreadId::new();
    app.ensure_thread_channel(child);
    app.activate_thread_channel(child).await;
    assert!(app.matches_focus_session(&key, &second.to_string()));
    // A TUI resumed directly into a child also qualifies for its known root.
    let thread = serde_json::from_value(serde_json::json!({
        "id": child.to_string(), "sessionId": second.to_string(), "parentThreadId": second.to_string(),
        "preview": "Child", "ephemeral": false, "modelProvider": "openai",
        "createdAt": 1, "updatedAt": 2, "status": {"type": "idle"},
        "cwd": app.config.cwd, "cliVersion": "0.0.0", "source": "cli", "turns": []
    }))?;
    app.agents_overview
        .model
        .threads
        .insert(child, Some(thread));
    app.enqueue_primary_thread_session(
        test_thread_session(child, app.config.cwd.to_path_buf()),
        Vec::new(),
    )
    .await?;
    assert_eq!(
        [
            app.matches_focus_session(&key, &first.to_string()),
            app.matches_focus_session(&key, &second.to_string())
        ],
        [false, true]
    );
    let other = Endpoint::from(&crate::resolve_remote_addr("ws://localhost:5001")?);
    assert!(!app.matches_focus_session(&other, &second.to_string()));
    app.app_server_target = AppServerTarget::Embedded;
    assert!(!app.matches_focus_session(&key, &second.to_string()));
    Ok(())
}
