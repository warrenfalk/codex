//! A live notes-only reader using the standard pager navigation and close keys.

use super::*;
use crate::history_cell::sanitize_user_text;
use crate::notes::NotesState;
use crate::terminal_hyperlinks::HyperlinkParagraph;
use ratatui::style::Style;

struct NoteRenderable {
    cell: Box<dyn HistoryCell>,
}

impl Renderable for NoteRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_scrolled(area, buf, /*scroll_offset*/ 0);
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        let lines = self.cell.transcript_hyperlink_lines(area.width);
        HyperlinkParagraph::new(&lines, Style::default())
            .scroll(scroll_offset)
            .render(area, buf);
        true
    }

    fn desired_height(&self, width: u16) -> u16 {
        let lines = self.cell.transcript_hyperlink_lines(width);
        HyperlinkParagraph::new(&lines, Style::default())
            .line_count(width)
            .try_into()
            .unwrap_or(u16::MAX)
    }
}

pub(crate) struct NotesOverlay {
    pager: StaticOverlay,
    revision: Option<(uuid::Uuid, u64)>,
}

impl NotesOverlay {
    pub(crate) fn new(notes: &NotesState, mut keymap: PagerKeymap) -> Self {
        keymap
            .close
            .insert(/*index*/ 0, key_hint::plain(KeyCode::Esc));
        let mut overlay = Self {
            pager: StaticOverlay::with_renderables(Vec::new(), String::new(), keymap),
            revision: None,
        };
        overlay.sync(notes);
        overlay
    }

    pub(crate) fn sync(&mut self, notes: &NotesState) {
        let revision = (notes.generation, notes.revision);
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        self.pager.view.title = if notes.loading || notes.error.is_some() {
            "Notes to self".to_string()
        } else {
            format!("Notes to self ({})", notes.entries.len())
        };
        let mut renderables: Vec<Box<dyn Renderable>> = Vec::new();
        if notes.loading {
            renderables.push(Box::new(Line::from("Loading notes…".dim())));
        } else if let Some(error) = &notes.error {
            renderables.push(Box::new(
                Paragraph::new(format!(
                    "Could not load all notes: {}\nClose and reopen /nts to retry.",
                    sanitize_user_text(error.as_str().into())
                ))
                .wrap(Wrap { trim: false }),
            ));
        } else if notes.entries.is_empty() {
            renderables.push(Box::new(
                Paragraph::new("No notes yet. Add one with /nts <note>.")
                    .wrap(Wrap { trim: false }),
            ));
        }
        for note in &notes.entries {
            renderables.push(Box::new(NoteRenderable {
                cell: Box::new(crate::history_cell::new_note_to_self(
                    sanitize_user_text(note.text.as_str().into()).into_owned(),
                )),
            }));
        }
        self.pager.view.renderables = renderables;
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        self.pager.handle_event(tui, event)
    }

    pub(crate) fn is_done(&self) -> bool {
        self.pager.is_done()
    }
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;
