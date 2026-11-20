//! Intercept Markdown links activated in the interactive conversation.

use super::App;
use super::desktop_links;
use crate::file_links::markdown_link_path;
use crate::pager_overlay::MarkdownOverlay;
use crate::pager_overlay::Overlay;
use crate::tui::Tui;

impl App {
    pub(crate) fn try_open_markdown_link(&mut self, tui: &mut Tui, url: &str) -> bool {
        // A workspace path on another machine may name a different file on the TUI host.
        if crate::uses_remote_workspace_or_environment(
            &self.app_server_target,
            &self.environment_manager,
        ) || !matches!(self.overlay, None | Some(Overlay::Transcript(_)))
        {
            return false;
        }
        let Some(path) = markdown_link_path(url) else {
            return false;
        };

        let _ = tui.enter_alt_screen();
        let events = self.app_event_tx.clone();
        let frames = tui.frame_requester();
        let preview = Box::new(MarkdownOverlay::new(
            path,
            url.to_string(),
            self.keymap.pager.clone(),
            frames.clone(),
            move |url| desktop_links::open(url, events.clone()),
        ));
        if let Some(Overlay::Transcript(transcript)) = &mut self.overlay {
            transcript.markdown_preview = Some(preview);
        } else {
            self.overlay = Some(Overlay::Markdown(preview));
        }
        frames.schedule_frame();
        true
    }
}

#[cfg(test)]
#[path = "markdown_preview_tests.rs"]
mod tests;
