//! Exercise copy entry through the app dispatcher, including an empty transcript.

use super::*;
use crate::app::owned_transcript::tests::buffer_text;

#[tokio::test]
async fn shortcut_copy_feedback_survives_clean_view_without_progress_or_history() -> Result<()> {
    use crate::clipboard_copy::CopyStatus;

    let (mut app, mut rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.transcript_mode = crate::transcript_mode::TranscriptMode::Owned;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(/*owned*/ true)?;
    let size = Size::new(/*width*/ 80, /*height*/ 12);
    tui.screen_size_for_event(&TuiEvent::Resize(size))?;
    let mut snapshots = Vec::new();
    for clean in [false, true] {
        app.apply_clean_scrollback_mode(&mut tui, clean);
        for (label, result) in [
            ("current input", Ok(CopyStatus::Confirmed)),
            ("last message", Ok(CopyStatus::Unconfirmed)),
            ("picker target", Err("clipboard unavailable".into())),
        ] {
            app.show_copy_result(label, /*characters*/ 24, Ok(CopyStatus::Pending(1)));
            app.transcript_view.finish_copy(
                &app.transcript_cells,
                &(1, result),
                /*current*/ true,
            );
            app.render_owned_transcript(&mut tui, size)?;
            snapshots.push(format!(
                "clean={clean}, {label}\n{}",
                buffer_text(crate::custom_terminal::test_support::last_rendered_buffer(
                    &tui.terminal
                ))
            ));
        }
    }
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::InsertHistoryCell(_)))
    );
    insta::assert_snapshot!(snapshots.join("\n\n"));
    Ok(())
}

#[tokio::test]
async fn empty_owned_copy_reports_feedback() -> Result<()> {
    let (mut app, mut rx, _op_rx) = make_test_app_with_channels().await;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let guard = Arc::new(crate::copy_input_guard::CopyInputGuard(
        app.app_event_tx.clone(),
    ));
    app.handle_event(
        &mut tui,
        &mut app_server,
        AppEvent::SelectTranscriptCopy { guard },
    )
    .await?;
    let lines = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::InsertHistoryCell(cell) => Some(cell.display_lines(/*width*/ 80)),
            _ => None,
        })
        .flatten()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(lines, @"• Nothing to copy");
    Ok(())
}
