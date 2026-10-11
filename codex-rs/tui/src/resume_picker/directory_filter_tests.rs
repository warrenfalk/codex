use super::super::tests::make_row;
use super::super::tests::page;
use super::super::tests::page_only_loader;
use super::super::*;
use pretty_assertions::assert_eq;
use std::sync::Mutex;

#[test]
fn initial_scope_respects_repository_availability_and_all_override() {
    let cwd = Path::new("/project");
    use SessionFilterMode::All;
    use SessionFilterMode::Cwd;
    use SessionFilterMode::Repo;

    for (show_all, filter_cwd, repo_available, resume_default, fork_default) in [
        (false, Some(cwd), true, Cwd, Repo),
        (false, Some(cwd), false, Cwd, Cwd),
        (true, Some(cwd), true, All, All),
        (true, Some(cwd), false, All, All),
        (false, None, false, All, All),
    ] {
        for (action, expected) in [
            (SessionPickerAction::Resume, resume_default),
            (SessionPickerAction::Fork, fork_default),
        ] {
            assert_eq!(
                SessionFilterMode::from_show_all(show_all, filter_cwd, repo_available, action),
                expected,
            );
        }
    }
}

#[tokio::test]
async fn arrows_cycle_available_scopes_in_both_directions() {
    for repo_available in [false, true] {
        for action in [SessionPickerAction::Resume, SessionPickerAction::Fork] {
            let requests = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&requests);
            let cwd = PathBuf::from("/project");
            let mut state = PickerState::new(
                FrameRequester::test_dummy(),
                page_only_loader(move |request| sink.lock().unwrap().push(request)),
                ProviderFilter::Any,
                /*show_all*/ true,
                Some(cwd.clone()),
                action,
            );
            state.repo_filter_available = repo_available;
            let forward = if repo_available {
                vec![
                    SessionFilterMode::Cwd,
                    SessionFilterMode::Repo,
                    SessionFilterMode::All,
                ]
            } else {
                vec![SessionFilterMode::Cwd, SessionFilterMode::All]
            };
            let backward = if repo_available {
                vec![
                    SessionFilterMode::Repo,
                    SessionFilterMode::Cwd,
                    SessionFilterMode::All,
                ]
            } else {
                vec![SessionFilterMode::Cwd, SessionFilterMode::All]
            };
            for (key, expected) in [(KeyCode::Right, forward), (KeyCode::Left, backward)] {
                requests.lock().unwrap().clear();
                for _ in &expected {
                    state.handle_key(KeyEvent::from(key)).await.unwrap();
                }
                assert_eq!(
                    requests
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|request| {
                            (
                                request.filter_mode,
                                request.cwd_filter.clone(),
                                request.cursor.is_none(),
                            )
                        })
                        .collect::<Vec<_>>(),
                    expected
                        .into_iter()
                        .map(|mode| {
                            (
                                mode,
                                (mode != SessionFilterMode::All).then(|| cwd.clone()),
                                true,
                            )
                        })
                        .collect::<Vec<_>>(),
                );
            }
        }
    }
}

#[test]
fn exact_cwd_and_repo_keep_distinct_subdirectory_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let primary = root.path().join("primary");
    let linked = root.path().join("linked");
    let unrelated = root.path().join("unrelated");
    let admin = primary.join(".git/worktrees/linked");
    for dir in [
        &admin,
        &primary.join("src"),
        &linked.join("src"),
        &unrelated,
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(primary.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(admin.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        admin.join("gitdir"),
        linked.join(".git").display().to_string(),
    )
    .unwrap();
    std::fs::write(linked.join(".git"), format!("gitdir: {}", admin.display())).unwrap();
    let primary = dunce::canonicalize(primary).unwrap();
    let linked = dunce::canonicalize(linked).unwrap();
    let cwd = primary.join("src");
    let linked_cwd = linked.join("src");
    let rows: Vec<_> = [cwd.clone(), linked_cwd.clone(), primary, linked, unrelated]
        .into_iter()
        .enumerate()
        .map(|(index, cwd)| {
            let mut row = make_row(&format!("{index}.jsonl"), "2025-01-01T00:00:00Z", "Session");
            row.cwd = Some(cwd);
            row
        })
        .collect();
    let mut state = PickerState::new(
        FrameRequester::test_dummy(),
        page_only_loader(|_| {}),
        ProviderFilter::Any,
        /*show_all*/ false,
        Some(cwd.clone()),
        SessionPickerAction::Resume,
    );
    state.repo_filter_available = true;
    let mut backend_filter = PageCwdFilter::default();
    for (mode, expected_rows, expected_filter) in [
        (
            SessionFilterMode::Repo,
            rows[..2].to_vec(),
            Some(ThreadListCwdFilter::Many(vec![
                cwd.display().to_string(),
                linked_cwd.display().to_string(),
            ])),
        ),
        (
            SessionFilterMode::Cwd,
            rows[..1].to_vec(),
            Some(ThreadListCwdFilter::One(cwd.display().to_string())),
        ),
        (SessionFilterMode::All, rows.clone(), None),
    ] {
        state.filter_mode = mode;
        state.start_initial_load();
        state.ingest_page(page(
            rows.clone(),
            /*next_cursor*/ None,
            rows.len(),
            /*reached_scan_cap*/ false,
        ));
        assert_eq!(state.filtered_rows, expected_rows);
        assert_eq!(
            backend_filter.for_request(
                /*cursor*/ None,
                state.active_cwd_filter().as_deref(),
                /*uses_remote_filesystem*/ false,
                mode,
            ),
            expected_filter
        );
    }
}

#[tokio::test]
async fn scope_changes_preserve_search_and_index_fallback_without_stale_pages() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&requests);
    let cwd = PathBuf::from("/project");
    let mut state = PickerState::new(
        FrameRequester::test_dummy(),
        page_only_loader(move |request| sink.lock().unwrap().push(request)),
        ProviderFilter::Any,
        /*show_all*/ false,
        Some(cwd.clone()),
        SessionPickerAction::Resume,
    );
    state.repo_filter_available = true;
    state.filter_mode = SessionFilterMode::Repo;
    state.initial_page_mode = PageLoadMode::StateDbOnly;
    state.status = SessionStatus::Archived;
    state.sort_key = ThreadSortKey::CreatedAt;
    state.set_query("Needle".into());
    state.start_initial_load();
    let stale_request = requests.lock().unwrap().last().unwrap().clone();
    state
        .handle_key(KeyEvent::from(KeyCode::Left))
        .await
        .unwrap();
    let current_request = requests.lock().unwrap().last().unwrap().clone();
    let mut row = make_row("session.jsonl", "2025-01-01T00:00:00Z", "Needle");
    row.cwd = Some(cwd.clone());
    state
        .handle_background_event(BackgroundEvent::Page {
            request_token: stale_request.request_token,
            search_token: stale_request.search_token,
            page: Ok(page(
                vec![row.clone()],
                Some("old-cursor"),
                /*num_scanned_files*/ 1,
                /*reached_scan_cap*/ false,
            )),
        })
        .await
        .unwrap();
    assert!(state.all_rows.is_empty());
    assert_eq!(requests.lock().unwrap().len(), 2);
    state
        .handle_background_event(BackgroundEvent::Page {
            request_token: current_request.request_token,
            search_token: current_request.search_token,
            page: Ok(page(
                vec![],
                /*next_cursor*/ None,
                /*num_scanned_files*/ 0,
                /*reached_scan_cap*/ false,
            )),
        })
        .await
        .unwrap();
    let fallback = requests.lock().unwrap().last().unwrap().clone();
    state
        .handle_background_event(BackgroundEvent::Page {
            request_token: fallback.request_token,
            search_token: fallback.search_token,
            page: Ok(page(
                vec![row.clone()],
                Some("next-page"),
                /*num_scanned_files*/ 1,
                /*reached_scan_cap*/ false,
            )),
        })
        .await
        .unwrap();
    state.load_more_if_needed(LoadTrigger::Scroll);
    let actual: Vec<_> = requests
        .lock()
        .unwrap()
        .iter()
        .map(|request| {
            (
                request.filter_mode,
                request.cwd_filter.clone(),
                request.mode,
                request
                    .cursor
                    .as_ref()
                    .map(|PageCursor::AppServer(cursor)| cursor.clone()),
                request.status,
                request.sort_key,
            )
        })
        .collect();
    let expected: Vec<_> = [
        (SessionFilterMode::Repo, PageLoadMode::StateDbOnly, None),
        (SessionFilterMode::Cwd, PageLoadMode::StateDbOnly, None),
        (SessionFilterMode::Cwd, PageLoadMode::StoreDefault, None),
        (
            SessionFilterMode::Cwd,
            PageLoadMode::StoreDefault,
            Some("next-page"),
        ),
    ]
    .into_iter()
    .map(|(mode, load_mode, cursor)| {
        (
            mode,
            Some(cwd.clone()),
            load_mode,
            cursor.map(str::to_owned),
            SessionStatus::Archived,
            ThreadSortKey::CreatedAt,
        )
    })
    .collect();
    assert_eq!(actual, expected);
    assert_eq!(
        (state.query.as_str(), &state.filtered_rows),
        ("Needle", &vec![row])
    );
}

#[test]
fn picker_snapshots_show_each_scope_in_wide_and_compact_layouts() {
    use crate::custom_terminal::Terminal;
    use crate::test_backend::VT100Backend;

    let mut snapshots = Vec::new();
    for action in [SessionPickerAction::Resume, SessionPickerAction::Fork] {
        for selected_mode in [
            None,
            Some(SessionFilterMode::Cwd),
            Some(SessionFilterMode::Repo),
            Some(SessionFilterMode::All),
        ] {
            let mut state = PickerState::new(
                FrameRequester::test_dummy(),
                page_only_loader(|_| {}),
                ProviderFilter::Any,
                /*show_all*/ false,
                Some(PathBuf::from("/repo/current")),
                action,
            );
            state.repo_filter_available = true;
            let mode = selected_mode.unwrap_or_else(|| {
                SessionFilterMode::from_show_all(
                    /*show_all*/ false,
                    state.filter_cwd.as_deref(),
                    state.repo_filter_available,
                    action,
                )
            });
            state.filter_mode = mode;
            let spans = filter_control_spans(&state, /*compact*/ false);
            let selected = spans
                .iter()
                .find(|span| span.content.trim() == mode.label())
                .unwrap();
            assert_eq!(selected.style, crate::bottom_pane::active_tab_style());
            for width in [110, 38] {
                let mut terminal =
                    Terminal::with_options(VT100Backend::new(width, /*height*/ 10)).unwrap();
                terminal.set_viewport_area(Rect::new(
                    /*x*/ 0, /*y*/ 0, width, /*height*/ 10,
                ));
                layout::render(&mut terminal.get_frame(), &state);
                terminal.flush().unwrap();
                let screen = terminal
                    .backend()
                    .to_string()
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n");
                let selection = if selected_mode.is_some() {
                    "selected"
                } else {
                    "default"
                };
                snapshots.push(format!(
                    "{action:?}, {selection} {}, width {width}\n{screen}",
                    mode.label()
                ));
            }
        }
    }
    insta::assert_snapshot!(snapshots.join("\n"));
}
