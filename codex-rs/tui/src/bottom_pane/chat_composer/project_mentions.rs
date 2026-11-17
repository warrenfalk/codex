//! Async project results update only the currently visible mention query.

use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;

impl ChatComposer {
    pub(crate) fn on_project_search_result(
        &mut self,
        query: &str,
        result: Result<Vec<AbsolutePathBuf>, String>,
    ) {
        let Some(current) = self.current_mentions_v2_token() else {
            return;
        };
        if crate::project_mentions::is_path_query(query) && current != query {
            return;
        }
        if !crate::project_mentions::is_path_query(query)
            && crate::project_mentions::is_path_query(&current)
        {
            return;
        }
        let error = result
            .as_ref()
            .err()
            .map(|error| format!("Project discovery failed: {error}"));
        self.project_mentions = result.unwrap_or_default();
        self.refresh_mentions_v2_popup_candidates();
        if let ActivePopup::MentionV2(popup) = &mut self.popups.active {
            popup.set_project_error(error);
        }
    }
}

#[cfg(test)]
#[path = "project_mentions_tests.rs"]
mod tests;
