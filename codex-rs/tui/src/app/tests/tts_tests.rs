use super::*;
use codex_config::types::TtsConfig;
use codex_config::types::TtsMode;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn speech_stop_keys_take_priority_over_composer_overlay_and_offline_handlers() -> Result<()> {
    for context in ["composer", "overlay", "offline"] {
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let (mut app, _, _) = make_test_app_with_channels().await;
            let mut app_server = start_config_write_test_app_server(&app).await?;
            let mut tui = crate::tui::test_support::make_test_tui()?;
            app.chat_widget
                .apply_external_edit("Keep this draft".into());
            if context == "overlay" {
                app.open_transcript_overlay(&mut tui);
            }
            app.reconnect.offline = context == "offline";
            app.chat_widget.speech.set_mode(TtsMode::Final);
            // No yield before handling the stop key: exercise a queued playback request.
            app.chat_widget
                .speech
                .enqueue(
                    "turn",
                    "message",
                    "Speaking".into(),
                    &TtsConfig {
                        command: vec!["unused-speech-command".into()],
                        ..Default::default()
                    },
                    &app.app_event_tx,
                )
                .expect("queue speech");
            let result = app
                .handle_tui_event(&mut tui, &mut app_server, TuiEvent::Key(key))
                .await?;
            assert!(matches!(result, AppRunControl::Continue));
            assert_eq!(
                (
                    app.chat_widget.speech.is_speaking(),
                    app.chat_widget.speech.mode(),
                    app.chat_widget.composer_text_with_pending(),
                    app.overlay.is_some(),
                    app.backtrack.primed
                ),
                (
                    false,
                    TtsMode::Final,
                    "Keep this draft".into(),
                    context == "overlay",
                    false
                )
            );
            if context == "offline" && key.code == KeyCode::Char('c') {
                let result = app
                    .handle_tui_event(&mut tui, &mut app_server, TuiEvent::Key(key))
                    .await?;
                assert!(matches!(
                    result,
                    AppRunControl::Exit(ExitReason::UserRequested)
                ));
            }
        }
    }
    Ok(())
}
