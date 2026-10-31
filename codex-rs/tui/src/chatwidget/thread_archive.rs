//! Choose the navigation action to run only after the current chat is archived.

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::app_event::ArchiveMode;

impl ChatWidget {
    pub(super) fn dispatch_archive(&mut self, args: &str) {
        let mode = match args.trim() {
            "" | "new" => ArchiveMode::NewChat,
            "exit" => ArchiveMode::Exit,
            _ => {
                self.add_error_message("Usage: /archive [new|exit]".to_string());
                return;
            }
        };
        self.app_event_tx.send(AppEvent::ArchiveCurrentThread(mode));
    }
}
