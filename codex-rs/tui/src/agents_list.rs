//! Shared list semantics for the interactive agents dashboard and JSON snapshots.

use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadActiveFlag;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_protocol::protocol::SubAgentSource;
use std::path::PathBuf;

pub(crate) const PREVIEW_CHARS: usize = 512;

pub(crate) fn preview_text(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
        .filter(|ch| !ch.is_control())
        .take(PREVIEW_CHARS)
        .collect()
}

pub(crate) fn parent_id(thread: &Thread) -> Option<String> {
    thread
        .parent_thread_id
        .clone()
        .or_else(|| match &thread.source {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id, ..
            }) => Some(parent_thread_id.to_string()),
            _ => None,
        })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AgentsOverviewGroup {
    #[serde(rename = "needsInput")]
    NeedsYou,
    Working,
    Ready,
    Finished,
}

impl AgentsOverviewGroup {
    pub(crate) fn for_status(status: &ThreadStatus) -> Self {
        match status {
            ThreadStatus::Active { active_flags }
                if active_flags.contains(&ThreadActiveFlag::WaitingOnApproval)
                    || active_flags.contains(&ThreadActiveFlag::WaitingOnUserInput) =>
            {
                Self::NeedsYou
            }
            ThreadStatus::Active { .. } => Self::Working,
            ThreadStatus::Idle => Self::Ready,
            ThreadStatus::SystemError => Self::NeedsYou,
            ThreadStatus::NotLoaded => Self::Finished,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NeedsYou => "Needs input",
            Self::Working => "Working",
            Self::Ready => "Ready",
            Self::Finished => "Finished",
        }
    }
}

pub(crate) fn display_title(thread: &Thread) -> &str {
    let title = thread.name.as_deref().unwrap_or(&thread.preview);
    title.trim().lines().next().unwrap_or("Untitled task")
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct AgentsOverviewProjectGroup {
    pub(crate) key: (PathBuf, PathBuf),
    pub(crate) heading: PathBuf,
}

impl AgentsOverviewProjectGroup {
    pub(crate) fn for_thread(thread: &Thread, worktrees_enabled: bool) -> Self {
        if worktrees_enabled
            && let Some(identity) = codex_git_utils::repository_identity(thread.cwd.as_path())
        {
            Self {
                key: (
                    identity.common_dir.into_path_buf(),
                    identity.relative_cwd.clone(),
                ),
                heading: identity.primary_root.as_path().join(identity.relative_cwd),
            }
        } else {
            Self {
                key: (thread.cwd.to_path_buf(), PathBuf::new()),
                heading: thread.cwd.to_path_buf(),
            }
        }
    }
}

pub(crate) fn update_preview(thread: &mut Thread, turn: &Turn) {
    if let Some(ThreadItem::UserMessage { content, .. }) = turn.items.first() {
        thread.preview =
            crate::chatwidget::ChatWidget::user_message_display_from_inputs(content).message;
    }
}
