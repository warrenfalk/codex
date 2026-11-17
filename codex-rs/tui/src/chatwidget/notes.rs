//! Note history loading and the persistent composer indicator.

use super::*;
use crate::notes::NoteToSelf;
use crate::notes::NotesState;

impl ChatWidget {
    pub(crate) fn reset_notes(&mut self) {
        self.notes = NotesState::default();
        self.reload_notes();
    }

    pub(crate) fn reload_notes(&mut self) {
        if let Some(thread_id) = self.thread_id {
            let generation = self.notes.begin_load();
            self.app_event_tx.send(AppEvent::LoadNotes {
                thread_id,
                generation,
            });
        }
        self.refresh_session_indicators();
    }

    pub(super) fn record_note(&mut self, id: String, text: String) {
        self.notes.record(NoteToSelf { id, text });
        self.refresh_session_indicators();
    }

    pub(crate) fn finish_notes_load(
        &mut self,
        generation: uuid::Uuid,
        result: Result<Vec<NoteToSelf>, String>,
    ) {
        if self.notes.finish_load(generation, result) {
            self.refresh_session_indicators();
            self.request_redraw();
        }
    }
}
