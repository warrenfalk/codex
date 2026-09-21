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
