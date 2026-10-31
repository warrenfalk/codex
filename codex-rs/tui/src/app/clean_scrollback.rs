//! A session-local display filter over retained history in both transcript modes.
//! Canonical cells and their indexes remain unchanged for pagination and backtracking.

use super::*;
use crate::history_cell::HistoryVisibilityKind;

impl App {
    pub(crate) fn cell_visible_in_current_scrollback(&self, cell: &dyn HistoryCell) -> bool {
        !self.clean_scrollback_enabled
            || cell.history_visibility_kind() == HistoryVisibilityKind::Normal
    }

    pub(crate) fn apply_clean_scrollback_mode(&mut self, tui: &mut tui::Tui, enabled: bool) {
        self.clean_scrollback_enabled = enabled;
        self.deferred_history_lines.clear();
        self.transcript_view.set_clean_scrollback_enabled(enabled);
        if let Some(Overlay::Transcript(overlay)) = &mut self.overlay {
            overlay.set_clean_scrollback_enabled(enabled);
        }
        let terminal_width = tui.terminal.last_known_screen_size.into();
        if let Err(err) = self.reflow_transcript_now(tui, terminal_width) {
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
