//! Dispatch clipboard key actions in order and apply completions to their selection owner.

use super::*;
use crate::chatwidget::KeyEventAction;
use crate::clipboard_copy::CopyFormat;
use crate::clipboard_copy::worker::PasteSource;
use crate::clipboard_paste::PasteImageError;

impl App {
    pub(super) fn show_copy_result(
        &mut self,
        label: &str,
        characters: usize,
        result: crate::clipboard_copy::worker::CopyResult,
    ) {
        if self.local_settings.transcript_mode.is_owned() {
            self.transcript_view.show_copy_feedback(&result, characters);
        } else {
            self.chat_widget.show_copy_result(label, result);
        }
    }

    pub(super) fn handle_clipboard_key_action(
        &mut self,
        tui: &mut tui::Tui,
        action: KeyEventAction,
    ) {
        match action {
            KeyEventAction::None => {}
            KeyEventAction::CopyLastResponse(text) => {
                let characters = text.chars().count();
                let result = tui
                    .clipboard
                    .copy(text, CopyFormat::Markdown, tui.frame_requester());
                self.show_copy_result("last message", characters, result);
            }
            KeyEventAction::CopyComposerText(text) => {
                let characters = text.chars().count();
                let result = tui
                    .clipboard
                    .copy(text, CopyFormat::PlainText, tui.frame_requester());
                self.show_copy_result("current input", characters, result);
            }
            KeyEventAction::PasteImage { text_fallback } => {
                if text_fallback.is_some()
                    && self
                        .right_click_paste_target(tui, PasteSource::Clipboard, text_fallback)
                        .is_none()
                {
                    return;
                }
                // Finish paste before the next key, but never wait on the copy worker's lock.
                if tui.clipboard.is_busy() || self.pending_right_click_paste.is_some() {
                    self.chat_widget
                        .add_info_message("Clipboard busy; try again".into(), /*hint*/ None);
                } else {
                    match self.chat_widget.paste_image() {
                        Ok(()) => {}
                        Err(PasteImageError::NoImage(_)) if text_fallback.is_some() => {
                            self.start_clipboard_text_paste(
                                tui,
                                PasteSource::Clipboard,
                                text_fallback,
                            );
                        }
                        Err(err) => {
                            tracing::warn!("failed to paste image: {err}");
                            self.chat_widget
                                .add_error_message(format!("Failed to paste image: {err}"));
                        }
                    }
                }
            }
        }
    }

    pub(super) fn finish_clipboard(&mut self, tui: &mut tui::Tui, event: &TuiEvent) {
        let current = tui.is_owned_screen() && self.overlay.is_none();
        if let Some(completion) = tui.clipboard.poll() {
            if let Some(characters) = self.chat_widget.finish_clipboard(completion, current) {
                self.transcript_view
                    .show_copy_feedback(&completion.1, characters);
            }
            {
                let mut selection = self.chat_widget.rendered_selection.borrow_mut();
                let crate::rendered_selection::RenderedSelection { view, cells, .. } =
                    &mut *selection;
                view.finish_copy(cells, completion, current);
            }
            let follow =
                self.transcript_view
                    .finish_copy(&self.transcript_cells, completion, current);
            if follow == Some(true) {
                if self.backtrack.overlay_preview_active {
                    self.close_transcript_overlay(tui);
                }
                self.transcript_view.jump_to_latest();
            }
        }
        if !current || matches!(event, TuiEvent::FocusLost | TuiEvent::Resume) {
            self.transcript_view.cancel_primary();
            self.chat_widget.cancel_composer_primary();
            self.chat_widget
                .rendered_selection
                .borrow_mut()
                .view
                .cancel_primary();
        }
    }
}
