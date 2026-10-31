//! Personal notes retained independently of the bounded, visible transcript.

use std::collections::HashSet;

use ratatui::style::Stylize;
use ratatui::text::Line;
use tokio::task::AbortHandle;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NoteToSelf {
    pub(crate) id: String,
    pub(crate) text: String,
}

#[derive(Default)]
pub(crate) struct NotesState {
    pub(crate) entries: Vec<NoteToSelf>,
    pub(crate) generation: Uuid,
    pub(crate) revision: u64,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) task: Option<AbortHandle>,
}

impl Drop for NotesState {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl NotesState {
    pub(crate) fn begin_load(&mut self) -> Uuid {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.generation = Uuid::new_v4();
        self.loading = true;
        self.error = None;
        self.revision = self.revision.wrapping_add(1);
        self.generation
    }

    pub(crate) fn record(&mut self, note: NoteToSelf) {
        if !self.entries.iter().any(|entry| entry.id == note.id) {
            self.entries.insert(/*index*/ 0, note);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(crate) fn finish_load(
        &mut self,
        generation: Uuid,
        result: Result<Vec<NoteToSelf>, String>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.task = None;
        self.loading = false;
        match result {
            Ok(notes) => {
                let mut seen = HashSet::new();
                let notes = notes
                    .into_iter()
                    .filter(|note| seen.insert(note.id.clone()))
                    .collect::<Vec<_>>();
                // Notes received live after the history request belong before its newest item.
                self.entries.retain(|note| !seen.contains(&note.id));
                self.entries.extend(notes);
            }
            Err(error) => self.error = Some(error),
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub(crate) fn indicator(&self) -> Option<Line<'static>> {
        let count = self.entries.len();
        let text = if self.error.is_some() {
            "Notes unavailable · /nts to retry".to_string()
        } else if count == 0 {
            return None;
        } else if self.loading {
            "Notes to self · /nts to view".to_string()
        } else if count == 1 {
            "1 note to self · /nts to view".to_string()
        } else {
            format!("{count} notes to self · /nts to view")
        };
        Some(Line::from(text.magenta()))
    }
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;
