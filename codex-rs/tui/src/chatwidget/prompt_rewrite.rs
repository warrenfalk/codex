//! Composer-facing prompt-rewrite operations and transient status messages.

use super::*;
use crate::prompt_rewrite::PromptRewriteRequest;
use crate::prompt_rewrite::PromptRewriteUnavailable;

const PROMPT_REWRITE_FLASH_DURATION: Duration = Duration::from_secs(4);

impl ChatWidget {
    /// Composer completion popups do not block rewriting: their selected text and metadata are
    /// captured with the draft. Modal views still own keyboard input while they are open.
    pub(crate) fn can_start_prompt_rewrite(&self) -> bool {
        self.bottom_pane.composer_input_enabled() && !self.bottom_pane.has_active_view()
    }

    pub(crate) fn prepare_prompt_rewrite_request(
        &mut self,
        parent_thread_id: ThreadId,
    ) -> Result<PromptRewriteRequest, PromptRewriteUnavailable> {
        let original_draft = self.bottom_pane.prepare_prompt_rewrite_snapshot();
        let recognized_slash_command = self
            .bottom_pane
            .recognizes_prompt_rewrite_excluded_slash_command();
        PromptRewriteRequest::new(parent_thread_id, original_draft, recognized_slash_command)
    }

    pub(crate) fn apply_prompt_rewrite_result(
        &mut self,
        request: &PromptRewriteRequest,
        model_output: &str,
    ) -> Result<bool, String> {
        let rewritten = request.restore_model_output(model_output)?;
        Ok(self
            .bottom_pane
            .apply_prompt_rewrite(&request.original_draft, rewritten))
    }

    pub(crate) fn show_prompt_rewrite_unavailable(
        &mut self,
        unavailable: PromptRewriteUnavailable,
    ) {
        let message = match unavailable {
            PromptRewriteUnavailable::Empty => "Nothing to rewrite.",
            PromptRewriteUnavailable::ShellCommand => "Shell commands are not rewritten.",
            PromptRewriteUnavailable::SlashCommand => "Slash commands are not rewritten.",
            PromptRewriteUnavailable::InvalidStructuredDraft => {
                "The draft's attachments or mentions are inconsistent."
            }
        };
        self.show_prompt_rewrite_flash(message, /*is_error*/ true);
    }

    pub(crate) fn show_prompt_rewrite_flash(&mut self, message: &str, is_error: bool) {
        let line = if is_error {
            Line::from(message.to_string().red())
        } else {
            Line::from(message.to_string().magenta())
        };
        self.bottom_pane
            .show_composer_flash(line, PROMPT_REWRITE_FLASH_DURATION);
    }
}

#[cfg(test)]
#[path = "prompt_rewrite_tests.rs"]
mod tests;
