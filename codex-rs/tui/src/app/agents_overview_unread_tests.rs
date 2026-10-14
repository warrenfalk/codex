use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn opening_unread_sessions_acknowledges_cold_and_already_displayed_threads() -> Result<()> {
    let (mut app, mut events, _operations) = make_test_app_with_channels().await;
    trust_fixture_folders(&mut app);
    std::fs::write(
        app.local_settings.user_config_path.as_path(),
        "[tui]\nresume_cwd = \"session\"\n",
    )?;
    let id = ThreadId::from_string(
        &app_test_support::create_fake_rollout(
            app.config.codex_home.as_path(),
            "2025-01-05T12-00-00",
            "2025-01-05T12:00:00Z",
            "Unread task",
            Some(&app.config.model_provider_id),
            /*git_info*/ None,
        )
        .expect("saved rollout"),
    )?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    server.thread_unread_set(id, /*unread*/ true).await?;
    assert!(
        server
            .thread_read(id, /*include_turns*/ false)
            .await?
            .unread
    );
    let mut tui = crate::tui::test_support::make_test_tui()?;
    Box::pin(app.select_agents_overview_thread(&mut tui, &mut server, id)).await?;
    let messages = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::InsertHistoryCell(cell) => Some(
                cell.display_lines(/*width*/ 140)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(app.current_displayed_thread_id(), Some(id), "{messages}");
    assert!(
        !server
            .thread_read(id, /*include_turns*/ false)
            .await?
            .unread
    );
    server.thread_unread_set(id, /*unread*/ true).await?;
    // A second explicit open must acknowledge even without an up-to-date list cache.
    Box::pin(app.select_agents_overview_thread(&mut tui, &mut server, id)).await?;
    assert!(
        !server
            .thread_read(id, /*include_turns*/ false)
            .await?
            .unread
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn unread_priority_and_filter_preserve_active_family_status() {
    let mut app = make_test_app().await;
    app.agents_overview.view_state.lock().unwrap().grouping = AgentsOverviewGrouping::Status;
    let mut threads = Vec::new();
    for (id, title, status, unread) in [
        (1, "Acknowledged", ThreadStatus::Idle, false),
        (2, "Unread result", ThreadStatus::Idle, true),
        (
            3,
            "Still working",
            ThreadStatus::Active {
                active_flags: Vec::new(),
            },
            true,
        ),
        (
            4,
            "Approval needed",
            ThreadStatus::Active {
                active_flags: vec![ThreadActiveFlag::WaitingOnApproval],
            },
            true,
        ),
        (5, "Unloaded unread", ThreadStatus::NotLoaded, true),
        (6, "Working child", ThreadStatus::Idle, true),
    ] {
        let mut thread = overview_thread(
            ThreadId::from_u128(id),
            /*parent_thread_id*/ None,
            title,
            status,
        );
        thread.unread = unread;
        threads.push(thread);
    }
    threads.push(overview_thread(
        ThreadId::from_u128(/*value*/ 7),
        Some(ThreadId::from_u128(/*value*/ 6)),
        "Child output",
        ThreadStatus::Active {
            active_flags: Vec::new(),
        },
    ));
    let view = app.agents_overview_view(threads.clone(), /*selected_thread_id*/ None);
    assert_eq!(
        view.rows
            .iter()
            .map(|row| (row.thread_id, row.group))
            .collect::<Vec<_>>(),
        vec![
            (
                ThreadId::from_u128(/*value*/ 4),
                AgentsOverviewGroup::NeedsYou
            ),
            (
                ThreadId::from_u128(/*value*/ 2),
                AgentsOverviewGroup::Unread
            ),
            (
                ThreadId::from_u128(/*value*/ 5),
                AgentsOverviewGroup::Unread
            ),
            (
                ThreadId::from_u128(/*value*/ 3),
                AgentsOverviewGroup::Working
            ),
            (
                ThreadId::from_u128(/*value*/ 6),
                AgentsOverviewGroup::Working
            ),
            (ThreadId::from_u128(/*value*/ 1), AgentsOverviewGroup::Ready),
        ]
    );
    app.chat_widget.show_bottom_pane_view(Box::new(view));
    insta::assert_snapshot!(
        "unread_priority",
        normalize_agent_center_snapshot(render_bottom_popup(&app.chat_widget, /*width*/ 110))
    );
    let mut view = app.agents_overview_view(threads, /*selected_thread_id*/ None);
    for _ in 0..2 {
        view.handle_key_event(KeyCode::Tab.into());
    }
    app.chat_widget.show_bottom_pane_view(Box::new(view));
    insta::assert_snapshot!(
        "unread_filter",
        normalize_agent_center_snapshot(render_bottom_popup(&app.chat_widget, /*width*/ 110))
    );
}
