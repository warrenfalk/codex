//! Chat widget hooks for side-conversation mode.
//!
//! App-level side-thread lifecycle lives in `app::side`; this module owns the
//! chat-surface pieces that side mode toggles, such as the composer placeholder,
//! footer label, and inline `/side` message submission behavior.

use super::*;
use crate::app_event::SideConversationCloseChoice;

pub(crate) const SIDE_CLOSE_PROMPT_VIEW_ID: &str = "side-conversation-close";

impl ChatWidget {
    pub(crate) fn submit_user_message_as_plain_user_turn(
        &mut self,
        user_message: UserMessage,
    ) -> Option<AppCommand> {
        self.submit_user_message_with_shell_escape_policy(user_message, ShellEscapePolicy::Disallow)
    }

    pub(crate) fn set_side_conversation_active(&mut self, active: bool) {
        self.active_side_conversation = active;
        let placeholder = if active {
            self.side_placeholder_text.clone()
        } else {
            self.normal_placeholder_text.clone()
        };
        self.bottom_pane.set_placeholder_text(placeholder);
        self.bottom_pane.set_side_conversation_active(active);
    }

    pub(crate) fn side_conversation_active(&self) -> bool {
        self.active_side_conversation
    }

    pub(crate) fn set_side_conversation_context_label(&mut self, label: Option<String>) {
        self.bottom_pane.set_side_conversation_context_label(label);
    }

    pub(crate) fn open_side_conversation_close_prompt(&mut self) {
        let item = |choice: SideConversationCloseChoice,
                    name: &'static str,
                    description: &'static str,
                    is_default: bool| {
            SelectionItem {
                name: name.to_string(),
                description: Some(description.to_string()),
                is_default,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::SideConversationCloseSelected(choice));
                })],
                dismiss_on_select: true,
                ..Default::default()
            }
        };
        self.show_selection_view(SelectionViewParams {
            view_id: Some(SIDE_CLOSE_PROMPT_VIEW_ID),
            title: Some("Close side conversation".to_string()),
            subtitle: Some("Choose what to do with this ephemeral side thread.".to_string()),
            items: vec![
                item(
                    SideConversationCloseChoice::Cancel,
                    "Cancel",
                    "Return to the side conversation.",
                    /*is_default*/ true,
                ),
                item(
                    SideConversationCloseChoice::Summarize,
                    "Summarize",
                    "Ask the side agent for a summary and save it to the parent.",
                    /*is_default*/ false,
                ),
                item(
                    SideConversationCloseChoice::Leave,
                    "Leave",
                    "Discard the side conversation without saving a summary.",
                    /*is_default*/ false,
                ),
            ],
            initial_selected_idx: Some(0),
            is_searchable: false,
            row_display: crate::bottom_pane::SelectionRowDisplay::Wrapped,
            col_width_mode: ColumnWidthMode::AutoVisible,
            on_cancel: Some(Box::new(|tx| {
                tx.send(AppEvent::SideConversationCloseSelected(
                    SideConversationCloseChoice::Cancel,
                ));
            })),
            ..Default::default()
        });
    }
}
