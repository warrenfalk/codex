//! Forward project discovery without changing session permissions while editing a draft.

use super::*;

impl ChatWidget {
    pub(crate) fn on_projects_attached(
        &mut self,
        roots: Vec<codex_utils_absolute_path::AbsolutePathBuf>,
        added: &[codex_utils_absolute_path::AbsolutePathBuf],
    ) {
        self.config.workspace_roots = roots.clone();
        self.config.permissions.set_workspace_roots(roots);
        self.add_info_message(
            format!(
                "Attached project directories for this session: {}",
                added
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some("The current permission profile and denied paths still apply.".to_string()),
        );
    }

    pub(crate) fn on_project_search_result(
        &mut self,
        query: &str,
        result: Result<Vec<codex_utils_absolute_path::AbsolutePathBuf>, String>,
    ) {
        let roots = self.config.permissions.user_visible_workspace_roots();
        self.bottom_pane.on_project_search_result(
            query,
            result.map(|paths| {
                paths
                    .into_iter()
                    .filter(|path| !roots.contains(path))
                    .collect()
            }),
        );
    }
}
