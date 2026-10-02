//! Keeps exact-directory discovery separate from linked-worktree discovery.

use std::path::Path;

use super::SessionPickerAction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SessionFilterMode {
    Cwd,
    Repo,
    All,
}

#[derive(Clone, Copy)]
pub(super) enum CycleDirection {
    Previous,
    Next,
}

impl SessionFilterMode {
    pub(super) fn from_show_all(
        show_all: bool,
        filter_cwd: Option<&Path>,
        repo_filter_available: bool,
        action: SessionPickerAction,
    ) -> Self {
        if show_all || filter_cwd.is_none() {
            Self::All
        } else if repo_filter_available && matches!(action, SessionPickerAction::Fork) {
            Self::Repo
        } else {
            Self::Cwd
        }
    }

    pub(super) fn available(
        filter_cwd: Option<&Path>,
        repo_filter_available: bool,
    ) -> &'static [Self] {
        if filter_cwd.is_none() {
            &[Self::All]
        } else if repo_filter_available {
            &[Self::Cwd, Self::Repo, Self::All]
        } else {
            &[Self::Cwd, Self::All]
        }
    }

    pub(super) fn cycle(
        self,
        direction: CycleDirection,
        filter_cwd: Option<&Path>,
        repo_filter_available: bool,
    ) -> Self {
        let available = Self::available(filter_cwd, repo_filter_available);
        let index = available.iter().position(|mode| *mode == self).unwrap_or(0);
        let next = match direction {
            CycleDirection::Previous => index + available.len() - 1,
            CycleDirection::Next => index + 1,
        };
        available[next % available.len()]
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Cwd => "Cwd",
            Self::Repo => "Repo",
            Self::All => "All",
        }
    }
}

#[cfg(test)]
#[path = "directory_filter_tests.rs"]
mod tests;
