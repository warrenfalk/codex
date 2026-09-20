//! Atomic prompt-rewrite capture and application for the composer.

use super::ChatComposer;
use super::ComposerDraftSnapshot;
use super::EditKind;
use crate::prompt_rewrite::RewrittenComposerDraft;

impl ChatComposer {
    /// Flush any buffered paste before capturing the content guarded by a rewrite request.
    pub(crate) fn prepare_prompt_rewrite_snapshot(&mut self) -> ComposerDraftSnapshot {
        self.flush_paste_burst_before_modified_input_for_undo();
        self.draft_snapshot()
    }

    pub(crate) fn recognizes_prompt_rewrite_excluded_slash_command(&self) -> bool {
        self.slash_input().recognizes_command(&self.current_text())
    }

    /// Apply a rewrite as one undoable edit if the content has not changed since capture.
    pub(crate) fn apply_prompt_rewrite(
        &mut self,
        expected: &ComposerDraftSnapshot,
        rewritten: RewrittenComposerDraft,
    ) -> bool {
        self.flush_paste_burst_before_modified_input_for_undo();
        if !self.draft_snapshot().has_same_content(expected) {
            return false;
        }

        self.set_current_cursor(expected.cursor);
        let undo_checkpoint = self.editor_undo_checkpoint();
        let started_vim_edit = self.begin_direct_vim_edit();
        let vim_history = std::mem::take(&mut self.vim_history);
        let local_image_paths = expected
            .local_images
            .iter()
            .map(|image| image.path.clone())
            .collect();
        self.set_remote_image_urls_raw(expected.remote_image_urls.clone());
        self.set_text_content_with_mention_bindings_raw(
            rewritten.text,
            rewritten.text_elements,
            local_image_paths,
            expected.mention_bindings.clone(),
        );
        self.set_pending_pastes_raw(expected.pending_pastes.clone());
        self.set_current_cursor(self.current_text().len());
        self.sync_popups();
        self.vim_history = vim_history;
        if started_vim_edit {
            self.finish_vim_edit();
        }
        self.record_edit_from(undo_checkpoint, EditKind::Atomic);
        true
    }
}
