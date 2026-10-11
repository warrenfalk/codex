//! Explicit directory scope for latest-session lookup, shared by resume and fork.

use crate::resume_source_kinds;
use codex_app_server_protocol::ThreadListCwdFilter;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadSortKey;
use std::path::Path;

/// Directory scope when selecting the latest session without opening a picker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LastSessionScope {
    Cwd,
    Repo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LatestSessionLookupMode {
    StateDbOnly,
    ScanAndRepair,
}

pub(crate) fn latest_session_lookup_params(
    uses_remote_filesystem: bool,
    model_provider: Option<String>,
    scope: LastSessionScope,
    cwd_filter: Option<&Path>,
    include_non_interactive: bool,
    lookup_mode: LatestSessionLookupMode,
) -> ThreadListParams {
    ThreadListParams {
        originators: None,
        cursor: None,
        limit: Some(1),
        sort_key: Some(ThreadSortKey::UpdatedAt),
        sort_direction: None,
        model_providers: model_provider.map(|provider| vec![provider]),
        source_kinds: Some(resume_source_kinds(include_non_interactive)),
        archived: Some(false),
        section_id: None,
        project_id: None,
        parent_thread_id: None,
        ancestor_thread_id: None,
        cwd: cwd_filter.map(|cwd| match scope {
            LastSessionScope::Cwd => ThreadListCwdFilter::One(cwd.to_string_lossy().into_owned()),
            LastSessionScope::Repo => crate::resume_picker::repository_cwd_filter(
                cwd,
                uses_remote_filesystem,
                /*worktrees_enabled*/ true,
            ),
        }),
        use_state_db_only: match lookup_mode {
            LatestSessionLookupMode::StateDbOnly => true,
            LatestSessionLookupMode::ScanAndRepair => false,
        },
        search_term: None,
    }
}

#[cfg(test)]
#[path = "latest_session_tests.rs"]
mod tests;
