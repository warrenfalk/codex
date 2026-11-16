//! Handle clipboard feedback and image paste after the app checks worker availability.
//! Pending feedback belongs to this widget, so switching conversations discards it.

use super::*;
use crate::clipboard_copy::CopyStatus;
use crate::clipboard_copy::worker::CopyResult;
use crate::clipboard_paste::PasteImageError;

const IMAGE_PASTE_REPEAT_WINDOW: Duration = Duration::from_millis(/*millis*/ 250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardPasteContext {
    Composer,
    DraftHandoff,
}

pub(super) struct PendingCopy {
    id: u64,
    label: String,
}

impl ChatWidget {
    /// Recognize image paste shortcuts, including presses consumed by repeat suppression.
    pub(super) fn image_paste_key_action(
        &mut self,
        key: KeyEvent,
        context: ClipboardPasteContext,
    ) -> Option<KeyEventAction> {
        let shift_insert = cfg!(windows) && key_hint::shift(KeyCode::Insert).is_press(key);
        let image_paste = key.kind == KeyEventKind::Press
            && ((matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&'v'))
                && key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
                || shift_insert);
        if !image_paste {
            if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Release) {
                self.suppress_image_paste_until = Instant::now();
            }
            return None;
        }
        let now = Instant::now();
        let suppressed = now < self.suppress_image_paste_until;
        self.suppress_image_paste_until = now + IMAGE_PASTE_REPEAT_WINDOW;
        if suppressed {
            return Some(KeyEventAction::None);
        }
        let text_fallback = (cfg!(windows)
            && (key.modifiers == KeyModifiers::CONTROL || shift_insert))
            .then_some(key.code);
        if text_fallback.is_some() && context == ClipboardPasteContext::Composer {
            self.bottom_pane.prepare_composer_clipboard_paste();
        }
        Some(KeyEventAction::PasteImage {
            text_fallback,
            context,
        })
    }

    pub(crate) fn paste_image(
        &mut self,
        context: ClipboardPasteContext,
        text_fallback: Option<KeyCode>,
    ) -> Result<(), PasteImageError> {
        let result = paste_image_to_temp_png();
        self.suppress_image_paste_until = Instant::now() + IMAGE_PASTE_REPEAT_WINDOW;
        if context == ClipboardPasteContext::DraftHandoff
            && text_fallback.is_some()
            && matches!(&result, Err(PasteImageError::NoImage(_)))
        {
            // Settle typing before capturing the asynchronous text target.
            self.bottom_pane.prepare_composer_clipboard_paste();
        }
        let (path, info) = result?;
        tracing::debug!(
            "pasted image size={}x{} format={}",
            info.width,
            info.height,
            info.encoded_format.label()
        );
        self.attach_image(path, context);
        Ok(())
    }

    /// Track pending copies silently and report their result without adding history.
    pub(crate) fn show_copy_result(&mut self, label: &str, result: CopyResult) {
        if let Ok(CopyStatus::Pending(id)) = result {
            self.pending_clipboard = Some(PendingCopy {
                id,
                label: label.into(),
            });
        }
        self.show_clipboard_flash(label, &result);
    }

    fn show_clipboard_flash(&mut self, label: &str, result: &CopyResult) {
        if matches!(result, Ok(CopyStatus::Pending(_))) {
            return;
        }
        let line = match result {
            Ok(status) => Line::from(status.message(label).dim()),
            Err(error) => Line::from(format!("Copy failed: {error}").red()),
        };
        self.bottom_pane
            .show_footer_flash(line, Duration::from_secs(/*secs*/ 3));
    }

    pub(crate) fn finish_clipboard(
        &mut self,
        completion: &(u64, CopyResult),
        composer_visible: bool,
    ) -> Option<usize> {
        if self
            .pending_clipboard
            .as_ref()
            .is_some_and(|pending| pending.id == completion.0)
            && let Some(pending) = self.pending_clipboard.take()
        {
            self.show_clipboard_flash(&pending.label, &completion.1);
        }
        self.bottom_pane
            .finish_composer_copy(completion, composer_visible)
    }
}
