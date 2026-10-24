use super::*;
use crate::chatwidget::tests::helpers::render_bottom_popup;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use codex_protocol::items::AsyncUserInputQuestion;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn tts_picker_selects_modes_and_indicator_survives_thread_switches() {
    let (mut chat, _, mut rx, _) = make_chatwidget_manual_with_sender().await;
    let startup_preferences = chat.local_settings.tui.tts.clone();
    chat.transcript.active_cell = None;
    chat.dispatch_command(SlashCommand::Speak);
    insta::assert_snapshot!("tts_mode_picker", render_bottom_popup(&chat, /*width*/ 80));
    chat.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let modes: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::SetTtsMode(mode) => Some(mode),
            _ => None,
        })
        .collect();
    assert_eq!(modes, vec![TtsMode::Final]);
    chat.set_tts_mode(TtsMode::Final);
    insta::assert_snapshot!(
        "tts_final_indicator",
        render_bottom_popup(&chat, /*width*/ 80)
    );
    chat.dispatch_command_with_args(SlashCommand::Speak, "progress-and-final".into(), Vec::new());
    chat.bottom_pane.set_task_running(/*running*/ true);
    insta::assert_snapshot!(
        "tts_progress_indicator_running",
        render_bottom_popup(&chat, /*width*/ 80)
    );
    insta::assert_snapshot!(
        "tts_progress_indicator_narrow",
        render_bottom_popup(&chat, /*width*/ 40)
    );
    chat.handle_speak_command("stop");
    assert_eq!(chat.speech.mode(), TtsMode::ProgressAndFinal);

    let (mut replacement, _, _, _) = make_chatwidget_manual_with_sender().await;
    assert_eq!(replacement.speech.mode(), startup_preferences.default_mode);
    replacement.inherit_tts(&mut chat);
    assert_eq!(replacement.speech.mode(), TtsMode::ProgressAndFinal);
    replacement.handle_speak_command("off");
    assert_eq!(replacement.speech.mode(), TtsMode::Off);
    assert_eq!(chat.local_settings.tui.tts, startup_preferences);
}

#[cfg(unix)]
#[tokio::test]
async fn only_live_selected_messages_and_questions_are_spoken_once() -> anyhow::Result<()> {
    use std::time::Duration;
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("spoken.txt");
    let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    chat.local_settings.tui.tts.command = vec![
        "sh".into(),
        "-c".into(),
        "cat >> \"$1\"; printf '\\nEND\\n' >> \"$1\"".into(),
        "fake-say".into(),
        output.to_string_lossy().into_owned(),
    ];
    let message = |id: &str, text: &str, phase| AgentMessageItem {
        id: id.into(),
        content: vec![AgentMessageContent::Text { text: text.into() }],
        phase,
        memory_citation: None,
        delivery: None,
        questions: None,
    };
    chat.on_agent_message_item_completed(
        message("off", "Silent", /*phase*/ None),
        "turn",
        /*from_replay*/ false,
    );
    chat.turn_lifecycle.start(Instant::now());
    chat.set_tts_mode(TtsMode::Final);
    chat.on_agent_message_delta("Streaming must not be spoken twice.".into());
    chat.on_agent_message_item_completed(
        message("progress", "Progress", Some(MessagePhase::Commentary)),
        "turn",
        /*from_replay*/ false,
    );
    chat.on_agent_message_item_completed(
        message("history", "Historical answer", /*phase*/ None),
        "turn",
        /*from_replay*/ true,
    );
    let answer = message(
        "final",
        "Final **answer**.\n\n```sh\nnot narrated\n```",
        Some(MessagePhase::FinalAnswer),
    );
    chat.on_agent_message_item_completed(answer.clone(), "turn", /*from_replay*/ false);
    chat.on_agent_message_item_completed(answer, "turn", /*from_replay*/ false);
    chat.on_agent_message_item_completed(
        message("legacy", "Legacy answer.", /*phase*/ None),
        "turn",
        /*from_replay*/ false,
    );
    let mut question = message(
        "question",
        "Choose a voice.\n- Charles\n- Alba",
        Some(MessagePhase::Commentary),
    );
    question.questions = Some(vec![AsyncUserInputQuestion {
        title: "Choose a voice.".into(),
        options: Some(vec!["Charles".into(), "Alba".into()]),
    }]);
    chat.on_agent_message_item_completed(question, "turn", /*from_replay*/ false);
    let params = ToolRequestUserInputParams {
        thread_id: chat.thread_id().unwrap().to_string(),
        turn_id: "turn".into(),
        item_id: "blocking-question".into(),
        questions: vec![codex_app_server_protocol::ToolRequestUserInputQuestion {
            id: "choice".into(),
            header: "Choice".into(),
            question: "Continue?".into(),
            is_other: false,
            is_secret: false,
            options: Some(vec![
                codex_app_server_protocol::ToolRequestUserInputOption {
                    label: "Yes".into(),
                    description: "Keep going.".into(),
                },
            ]),
        }],
        is_blocking: true,
        auto_resolution_ms: None,
    };
    chat.handle_server_request(
        ServerRequest::ToolRequestUserInput {
            request_id: codex_app_server_protocol::RequestId::Integer(1),
            params: params.clone(),
        },
        Some(ReplayKind::ThreadSnapshot),
    );
    chat.handle_server_request(
        ServerRequest::ToolRequestUserInput {
            request_id: codex_app_server_protocol::RequestId::Integer(1),
            params,
        },
        /*replay_kind*/ None,
    );
    chat.handle_thread_item(
        ThreadItem::Plan {
            id: "old-plan".into(),
            text: "Historical plan".into(),
        },
        "turn".into(),
        ThreadItemRenderSource::Replay(ReplayKind::ThreadSnapshot),
    );
    chat.transcript.plan_delta_buffer = "Proposed plan.".into();
    chat.handle_thread_item(
        ThreadItem::Plan {
            id: "plan".into(),
            text: String::new(),
        },
        "turn".into(),
        ThreadItemRenderSource::Live,
    );
    let expected = "Final answer.\nEND\nLegacy answer.\nEND\nChoose a voice.\nCharles\nAlba\nEND\nContinue?\nOption 1: Yes. Keep going.\nEND\nProposed plan.\nEND\n";
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(&output).is_ok_and(|text| text == expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(std::fs::read_to_string(&output)?, expected);
    chat.set_tts_mode(TtsMode::ProgressAndFinal);
    chat.on_agent_message_item_completed(
        message(
            "later-progress",
            "Now speaking progress.",
            Some(MessagePhase::Commentary),
        ),
        "turn",
        /*from_replay*/ false,
    );
    let expected = format!("{expected}Now speaking progress.\nEND\n");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(&output).is_ok_and(|text| text == expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(std::fs::read_to_string(output)?, expected);
    Ok(())
}

#[cfg(unix)]
async fn wait_until_silent(speech: &crate::tts::Speech) -> anyhow::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while speech.is_speaking() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

#[cfg(unix)]
fn recording_command(output: &std::path::Path) -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        "cat >> \"$1\"; printf '\\nEND\\n' >> \"$1\"".into(),
        "fake-say".into(),
        output.to_string_lossy().into_owned(),
    ]
}

#[cfg(unix)]
#[tokio::test]
async fn enabling_idle_speech_reads_the_latest_response_and_can_replay_it() -> anyhow::Result<()> {
    for mode in [TtsMode::Final, TtsMode::ProgressAndFinal] {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("spoken.txt");
        let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
        chat.local_settings.tui.tts.command = recording_command(&output);
        for (id, text) in [("older", "Older answer."), ("latest", "Latest **answer**.")] {
            chat.handle_thread_item(
                ThreadItem::AgentMessage {
                    id: id.into(),
                    text: text.into(),
                    phase: Some(MessagePhase::FinalAnswer),
                    memory_citation: None,
                    delivery: None,
                    questions: None,
                },
                "turn".into(),
                ThreadItemRenderSource::Replay(ReplayKind::ThreadSnapshot),
            );
        }
        assert!(!chat.speech.is_speaking());
        if mode == TtsMode::ProgressAndFinal {
            // Background MCP startup is not a pending conversation turn.
            chat.mcp_startup_status = Some(HashMap::new());
            chat.update_task_running_state();
        }
        chat.set_tts_mode(mode);
        wait_until_silent(&chat.speech).await?;
        assert_eq!(std::fs::read_to_string(&output)?, "Latest answer.\nEND\n");

        chat.set_tts_mode(mode);
        chat.set_tts_mode(TtsMode::ProgressAndFinal);
        assert!(!chat.speech.is_speaking());
        chat.set_tts_mode(TtsMode::Off);
        chat.set_tts_mode(mode);
        wait_until_silent(&chat.speech).await?;
        assert_eq!(
            std::fs::read_to_string(&output)?,
            "Latest answer.\nEND\nLatest answer.\nEND\n"
        );

        chat.set_tts_mode(TtsMode::Off);
        chat.handle_thread_item(
            ThreadItem::Plan {
                id: "plan".into(),
                text: "Latest plan.".into(),
            },
            "turn".into(),
            ThreadItemRenderSource::Replay(ReplayKind::ThreadSnapshot),
        );
        chat.set_tts_mode(mode);
        wait_until_silent(&chat.speech).await?;
        assert_eq!(
            std::fs::read_to_string(&output)?,
            "Latest answer.\nEND\nLatest answer.\nEND\nLatest plan.\nEND\n"
        );
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn enabling_speech_during_a_pending_or_running_turn_waits_for_new_messages()
-> anyhow::Result<()> {
    for agent_turn_running in [false, true] {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("spoken.txt");
        let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
        chat.local_settings.tui.tts.command = recording_command(&output);
        chat.transcript
            .record_agent_markdown("Previous answer.".into(), "Previous answer.".into());
        if agent_turn_running {
            chat.turn_lifecycle.start(Instant::now());
        } else {
            chat.input_queue.user_turn_pending_start = true;
        }
        chat.set_tts_mode(TtsMode::Final);
        assert!(!chat.speech.is_speaking());
        chat.on_agent_message_item_completed(
            AgentMessageItem {
                id: "new".into(),
                content: vec![AgentMessageContent::Text {
                    text: "New answer.".into(),
                }],
                phase: Some(MessagePhase::FinalAnswer),
                memory_citation: None,
                delivery: None,
                questions: None,
            },
            "turn",
            /*from_replay*/ false,
        );
        wait_until_silent(&chat.speech).await?;
        assert_eq!(std::fs::read_to_string(&output)?, "New answer.\nEND\n");
    }
    Ok(())
}

#[tokio::test]
async fn speech_stop_keys_preserve_the_draft_mode_and_running_turn() -> anyhow::Result<()> {
    for key in [
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let (mut chat, _, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane.set_task_running(/*running*/ true);
        chat.turn_lifecycle.start(Instant::now());
        chat.apply_external_edit("Keep this draft".into());
        chat.set_tts_mode(TtsMode::Final);
        // The key is handled before the worker can spawn this command.
        chat.local_settings.tui.tts.command = vec!["unused-speech-command".into()];
        chat.speak_text("turn", "message", "Speaking");
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..key
        };
        assert!(!chat.handle_speech_key(release));
        assert!(chat.speech.is_speaking());
        chat.handle_key_event(key);
        assert_eq!(
            (
                chat.speech.is_speaking(),
                chat.speech.mode(),
                chat.composer_text_with_pending(),
                chat.is_agent_turn_running()
            ),
            (false, TtsMode::Final, "Keep this draft".into(), true)
        );
        assert!(chat.quit_shortcut_expires_at.is_none());
        assert!(
            !std::iter::from_fn(|| ops.try_recv().ok())
                .any(|op| matches!(op, AppCommand::Interrupt))
        );
        assert!(
            !std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| matches!(event, AppEvent::Exit(_)))
        );
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_command_turns_off_speech_and_reports_the_error() -> anyhow::Result<()> {
    let (mut chat, _, mut rx, _) = make_chatwidget_manual_with_sender().await;
    chat.local_settings.tui.tts.command =
        vec!["sh".into(), "-c".into(), "cat >/dev/null; exit 7".into()];
    chat.set_tts_mode(TtsMode::Final);
    chat.speak_text("turn", "item", "Hello");
    let (generation, message) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(AppEvent::TtsFailed {
                generation,
                message,
            }) = rx.recv().await
            {
                break (generation, message);
            }
        }
    })
    .await?;
    chat.on_tts_failure(generation, message);
    assert_eq!(chat.speech.mode(), TtsMode::Off);
    let mut lines = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            lines.extend(
                cell.display_lines(/*width*/ 80)
                    .into_iter()
                    .map(|line| line.to_string()),
            );
        }
    }
    insta::assert_snapshot!("tts_command_failure", lines.join("\n"));
    Ok(())
}
