use super::*;
use crate::keymap::RuntimeKeymap;
use crate::terminal_hyperlinks::strip_osc8;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::cell::RefCell;
use std::rc::Rc;

fn render(preview: &mut MarkdownOverlay, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| preview.render(frame.area(), frame.buffer_mut()))
        .expect("draw");
    strip_osc8(&terminal.backend().to_string())
}

#[tokio::test]
async fn markdown_preview_renders_document_and_keeps_source_link_in_view() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("Reading guide.md");
    let paragraphs = (1..=30)
        .map(|number| format!("Paragraph number {number}.\n\n"))
        .collect::<String>();
    std::fs::write(
        &path,
        format!(
            "# Reading guide\n\nRead the **first** section.\n\n- One\n- Two\n\n\
             ```rust\nfn main() {{}}\n```\n\n\
             | Item | Status |\n| --- | --- |\n| Preview | Ready |\n\n\
             {paragraphs}"
        ),
    )?;
    let source_url = "cursor://file/tmp/Reading%20guide.md:8:3";
    let (frames, mut requests) = FrameRequester::test_channel();
    let opened = Rc::new(RefCell::new(Vec::new()));
    let mut preview = MarkdownOverlay::new(
        path,
        source_url.to_string(),
        RuntimeKeymap::defaults().pager,
        frames,
        {
            let opened = Rc::clone(&opened);
            move |url| opened.borrow_mut().push(url)
        },
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), requests.recv()).await?;
    insta::assert_snapshot!(
        "markdown_preview",
        render(&mut preview, /*width*/ 52, /*height*/ 23)
    );

    let mut tui = crate::tui::test_support::make_test_tui()?;
    preview.handle_event(&mut tui, TuiEvent::Key(KeyCode::End.into()))?;
    let screen = render(&mut preview, /*width*/ 36, /*height*/ 12);
    assert!(screen.contains("Paragraph number 30."));
    assert!(screen.contains("Open source (enter)"));
    let click = |kind| {
        TuiEvent::Mouse(MouseEvent {
            kind,
            column: preview.source_area.x,
            row: preview.source_area.y,
            modifiers: KeyModifiers::NONE,
        })
    };
    let press = click(MouseEventKind::Down(MouseButton::Left));
    let release = click(MouseEventKind::Up(MouseButton::Left));
    preview.handle_event(&mut tui, press)?;
    assert!(opened.borrow().is_empty());
    preview.handle_event(&mut tui, release)?;
    assert_eq!(*opened.borrow(), [source_url]);

    preview.handle_event(&mut tui, TuiEvent::Key(KeyCode::Esc.into()))?;
    assert!(preview.is_done());
    Ok(())
}

#[tokio::test]
async fn unreadable_markdown_keeps_the_source_action_available() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("missing.md");
    let source_url = "vscode://file/tmp/missing.md";
    let (frames, mut requests) = FrameRequester::test_channel();
    let opened = Rc::new(RefCell::new(Vec::new()));
    let mut preview = MarkdownOverlay::new(
        path,
        source_url.to_string(),
        RuntimeKeymap::defaults().pager,
        frames,
        {
            let opened = Rc::clone(&opened);
            move |url| opened.borrow_mut().push(url)
        },
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), requests.recv()).await?;
    let screen = render(&mut preview, /*width*/ 52, /*height*/ 10);
    assert!(screen.contains("Could not preview Markdown."));
    assert!(screen.contains("Open source (enter)"));

    let mut tui = crate::tui::test_support::make_test_tui()?;
    preview.handle_event(&mut tui, TuiEvent::Key(KeyCode::Enter.into()))?;
    assert_eq!(*opened.borrow(), [source_url]);
    Ok(())
}

#[test]
fn markdown_preview_rejects_large_or_non_file_documents() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("large.md");
    std::fs::File::create(&path)?.set_len(MAX_PREVIEW_BYTES + 1)?;
    assert!(
        read_markdown(&path)
            .unwrap_err()
            .to_string()
            .contains("256 KiB")
    );
    assert!(read_markdown(directory.path()).is_err());
    Ok(())
}
