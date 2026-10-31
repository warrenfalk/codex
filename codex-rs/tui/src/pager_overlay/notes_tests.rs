use super::*;
use crate::keymap::RuntimeKeymap;
use crate::notes::NoteToSelf;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn render(overlay: &mut NotesOverlay, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| overlay.pager.render(frame.area(), frame.buffer_mut()))
        .expect("draw");
    terminal.backend().to_string()
}

#[test]
fn notes_reader_shows_complete_multiline_notes_newest_first() {
    let mut notes = NotesState::default();
    notes.record(NoteToSelf {
        id: "old".to_string(),
        text: "Remember to check the edge cases.".to_string(),
    });
    notes.record(NoteToSelf {
        id: "new".to_string(),
        text: "Side conversation summary\n\nUse the existing pager navigation. Keep the full note visible, including longer lines and blank lines.".to_string(),
    });
    let mut overlay = NotesOverlay::new(&notes, RuntimeKeymap::defaults().pager);
    insta::assert_snapshot!(
        "notes_reader",
        render(&mut overlay, /*width*/ 65, /*height*/ 20)
    );
    insta::assert_snapshot!(
        "notes_reader_narrow",
        render(&mut overlay, /*width*/ 32, /*height*/ 23)
    );
}

#[test]
fn notes_reader_shows_empty_loading_and_failed_states() {
    let mut notes = NotesState::default();
    let mut overlay = NotesOverlay::new(&notes, RuntimeKeymap::defaults().pager);
    insta::assert_snapshot!(
        "notes_reader_empty",
        render(&mut overlay, /*width*/ 65, /*height*/ 10)
    );

    let generation = notes.begin_load();
    overlay.sync(&notes);
    insta::assert_snapshot!(
        "notes_reader_loading",
        render(&mut overlay, /*width*/ 65, /*height*/ 10)
    );

    notes.finish_load(generation, Err("connection lost".to_string()));
    overlay.sync(&notes);
    insta::assert_snapshot!(
        "notes_reader_failed",
        render(&mut overlay, /*width*/ 65, /*height*/ 10)
    );
}

#[tokio::test]
async fn notes_reader_navigates_and_closes_with_standard_pager_keys() -> Result<()> {
    let mut notes = NotesState::default();
    notes.record(NoteToSelf {
        id: "long".to_string(),
        text: (0..80).map(|line| format!("Line {line}\n")).collect(),
    });
    let mut overlay = NotesOverlay::new(&notes, RuntimeKeymap::defaults().pager);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    render(&mut overlay, /*width*/ 65, /*height*/ 20);
    for key in [KeyCode::Down, KeyCode::PageDown, KeyCode::End] {
        overlay.handle_event(
            &mut tui,
            TuiEvent::Key(KeyEvent::new(key, KeyModifiers::NONE)),
        )?;
        render(&mut overlay, /*width*/ 65, /*height*/ 20);
        assert!(overlay.pager.view.scroll_offset > 0);
    }
    for key in [KeyCode::Up, KeyCode::PageUp, KeyCode::Home] {
        overlay.handle_event(
            &mut tui,
            TuiEvent::Key(KeyEvent::new(key, KeyModifiers::NONE)),
        )?;
    }
    assert_eq!(overlay.pager.view.scroll_offset, 0);
    overlay.handle_event(
        &mut tui,
        TuiEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    )?;
    assert!(overlay.is_done());
    Ok(())
}
