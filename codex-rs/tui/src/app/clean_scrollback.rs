//! Clean scrollback filtering for committed transcript history.

use super::*;
use crate::history_cell::HistoryVisibilityKind;

impl App {
    pub(crate) fn cell_visible_in_current_scrollback(&self, cell: &dyn HistoryCell) -> bool {
        !self.clean_scrollback_enabled
            || cell.history_visibility_kind() == HistoryVisibilityKind::Normal
    }

    pub(crate) fn transcript_cells_for_current_scrollback(&self) -> Vec<Arc<dyn HistoryCell>> {
        self.transcript_cells
            .iter()
            .filter(|cell| self.cell_visible_in_current_scrollback(cell.as_ref()))
            .cloned()
            .collect()
    }

    pub(crate) fn scrollback_index_for_transcript_cell_index(
        &self,
        transcript_cell_index: usize,
    ) -> Option<usize> {
        let cell = self.transcript_cells.get(transcript_cell_index)?;
        if !self.cell_visible_in_current_scrollback(cell.as_ref()) {
            return None;
        }
        self.transcript_cells
            .iter()
            .take(transcript_cell_index.saturating_add(1))
            .filter(|cell| self.cell_visible_in_current_scrollback(cell.as_ref()))
            .count()
            .checked_sub(1)
    }

    pub(crate) fn sync_transcript_overlay_cells(&mut self) {
        let cells = self.transcript_cells_for_current_scrollback();
        if let Some(Overlay::Transcript(t)) = &mut self.overlay {
            t.replace_cells(cells);
        }
        if self.backtrack.overlay_preview_active && self.backtrack.nth_user_message != usize::MAX {
            self.apply_backtrack_selection_internal(self.backtrack.nth_user_message);
        }
    }

    pub(crate) fn apply_clean_scrollback_mode(&mut self, tui: &mut tui::Tui, enabled: bool) {
        self.clean_scrollback_enabled = enabled;
        self.deferred_history_lines.clear();
        self.sync_transcript_overlay_cells();
        if let Err(err) = self.reflow_transcript_now(tui) {
            tracing::warn!(
                error = %err,
                "failed to reflow transcript after clean scrollback toggle"
            );
            self.chat_widget
                .add_error_message(format!("Failed to redraw transcript: {err}"));
        }
        tui.frame_requester().schedule_frame();
    }

    pub(crate) fn toggle_clean_scrollback(&mut self, tui: &mut tui::Tui) {
        self.apply_clean_scrollback_mode(tui, !self.clean_scrollback_enabled);
    }
}
