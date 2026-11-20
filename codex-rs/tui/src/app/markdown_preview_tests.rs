use super::*;
use crate::history_cell::AgentMarkdownCell;
use crate::tui::TuiEvent;
use codex_config::types::UriBasedFileOpener;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use url::Url;

#[test]
fn markdown_links_resolve_encoded_paths_and_editor_locations() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("A guide #2.MD");
    let file = Url::from_file_path(&path).expect("file URI");
    for scheme in ["vscode", "vscode-insiders", "cursor", "windsurf"] {
        let editor = format!("{scheme}://file{}:12:4", file.path());
        assert_eq!(markdown_link_path(&editor), Some(path.clone()));
    }
    assert_eq!(markdown_link_path(file.as_str()), Some(path));
    for other in [
        "https://example.com/README.md",
        "file://server/share/README.md",
        "cursor://file//server/share/README.md",
        "vscode://settings/README.md",
        "vscode://file/tmp/example.rs:4",
    ] {
        assert_eq!(markdown_link_path(other), None);
    }
    Ok(())
}

#[tokio::test]
async fn clicking_markdown_in_a_conversation_opens_the_preview_and_escape_returns()
-> color_eyre::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("Markdown #example.md");
    std::fs::write(&path, "# File heading\n\nThe file has **styled** text.\n")?;
    let file = Url::from_file_path(&path).expect("file URI");
    let link = format!("vscode://file{}:2:3", file.path());

    for owned in [true, false] {
        let mut app = crate::app::test_support::make_test_app().await;
        crate::app::owned_transcript::tests::attach_thread(
            &mut app,
            codex_protocol::ThreadId::new(),
        );
        app.transcript_cells.push(Arc::new(
            AgentMarkdownCell::new_with_file_opener_and_inline_visualizations(
                format!("Read [the local guide]({file}#L2C3) before continuing."),
                directory.path(),
                UriBasedFileOpener::VsCode,
                /*inline_visualization_context*/ None,
            ),
        ));
        let mut app_server =
            Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(owned)?;
        if !owned {
            app.open_transcript_overlay(&mut tui);
        }
        app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
            .await?;
        let buffer = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
        let position = buffer
            .area
            .positions()
            .find(|&position| buffer[position].symbol().contains(&link))
            .expect("visible file link");

        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_tui_event(
                &mut tui,
                &mut app_server,
                TuiEvent::Mouse(MouseEvent {
                    kind,
                    column: position.x,
                    row: position.y,
                    modifiers: KeyModifiers::NONE,
                }),
            )
            .await?;
        }
        match &app.overlay {
            Some(Overlay::Markdown(_)) if owned => {}
            Some(Overlay::Transcript(transcript))
                if !owned && transcript.markdown_preview.is_some() => {}
            _ => panic!("expected a Markdown preview"),
        }

        app.handle_tui_event(
            &mut tui,
            &mut app_server,
            TuiEvent::Key(KeyCode::Esc.into()),
        )
        .await?;
        if owned {
            assert!(app.overlay.is_none());
        } else {
            assert!(
                matches!(&app.overlay, Some(Overlay::Transcript(t)) if t.markdown_preview.is_none())
            );
        }
        app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
            .await?;
        let restored = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
        assert!(
            restored
                .content()
                .iter()
                .any(|cell| cell.symbol().contains(&link))
        );
        if owned {
            tui.set_owned_screen(/*owned*/ false)?;
        } else {
            app.close_transcript_overlay(&mut tui);
        }
        app_server.shutdown().await?;
    }
    Ok(())
}
