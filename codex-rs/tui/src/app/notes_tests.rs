use super::*;
use crate::legacy_core::config::ConfigBuilder;
use app_test_support::create_fake_paginated_rollout;
use app_test_support::create_fake_rollout;
use app_test_support::rollout_path;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::NoteToSelfEvent;
use pretty_assertions::assert_eq;
use std::io::Write;

#[tokio::test]
async fn notes_reader_handles_a_new_thread_before_its_first_turn() -> Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let started = server
        .start_thread_with_session_start_source(
            &crate::local_settings::LocalSettings::from(&config),
            &config,
            /*session_start_source*/ None,
            /*remote_cwd_override*/ None,
            /*selected_profile*/ None,
        )
        .await?;
    assert_eq!(
        read_notes(server.request_handle(), started.session.thread_id).await?,
        Vec::new()
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn notes_reader_loads_all_notes_from_legacy_and_paginated_history() -> Result<()> {
    for history_mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let home = tempfile::tempdir()?;
        let config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .build()
            .await?;
        let create = match history_mode {
            ThreadHistoryMode::Legacy => create_fake_rollout,
            ThreadHistoryMode::Paginated => create_fake_paginated_rollout,
        };
        let id = create(
            home.path(),
            "2026-01-01T00-00-00",
            "2026-01-01T00:00:00Z",
            "Ordinary conversation text should not appear among notes",
            Some(&config.model_provider_id),
            /*git_info*/ None,
        )
        .expect("create rollout");
        let path = rollout_path(home.path(), "2026-01-01T00-00-00", &id);
        let start = std::fs::read_to_string(&path)?.lines().count();
        let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
        let expected = (0..HISTORY_ITEM_PAGE_LIMIT + 3)
            .map(|index| NoteToSelf {
                id: format!("note-{index}"),
                text: format!("Personal note {index}\nSecond line"),
            })
            .collect::<Vec<_>>();
        for (index, note) in expected.iter().enumerate() {
            let mut line = serde_json::json!({
                "timestamp": "2026-01-01T00:00:01Z",
                "type": "event_msg",
                "payload": EventMsg::NoteToSelf(NoteToSelfEvent {
                    note: note.text.clone(),
                    item_id: Some(note.id.clone()),
                    active_turn_id: None,
                }),
            });
            if history_mode == ThreadHistoryMode::Paginated {
                line["ordinal"] = serde_json::json!(start + index);
            }
            writeln!(file, "{line}")?;
        }
        drop(file);
        let mut app_server = crate::start_embedded_app_server_for_picker(&config).await?;
        let thread_id = ThreadId::from_string(&id)?;
        app_server
            .resume_thread(
                &crate::local_settings::LocalSettings::from(&config),
                config,
                thread_id,
                crate::app_server_session::ResumeModelSettings::RestoreFromThread,
            )
            .await?;
        let actual = read_notes(app_server.request_handle(), thread_id).await?;
        assert_eq!(
            actual,
            expected.into_iter().rev().collect::<Vec<_>>(),
            "{history_mode:?}"
        );
        app_server.shutdown().await?;
    }
    Ok(())
}
