//! Persistent session indicators share the notes reminder row above the input.

use super::ChatWidget;
use ratatui::style::Stylize;
use ratatui::text::Line;

impl ChatWidget {
    pub(crate) fn set_unread_indicator(&mut self, show_unread: bool) {
        let show_unread =
            show_unread && !self.bottom_pane.is_task_running() && !self.side_conversation_active();
        if self.status_state.unread_indicator != show_unread {
            self.status_state.unread_indicator = show_unread;
            self.refresh_session_indicators();
        }
    }

    pub(super) fn refresh_session_indicators(&mut self) {
        let mut indicator = self.notes.indicator();
        if self.status_state.unread_indicator
            && !self.bottom_pane.is_task_running()
            && !self.side_conversation_active()
        {
            let mut line = Line::from(vec!["unread".light_blue()]);
            if let Some(notes) = indicator {
                line.spans.push(" · ".dim());
                line.spans.extend(notes.spans);
            }
            indicator = Some(line);
        }
        self.bottom_pane.set_notes_indicator(indicator);
    }
}
