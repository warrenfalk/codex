//! Distinguish explicit project attachments from client configuration defaults.

use codex_utils_absolute_path::AbsolutePathBuf;

pub(crate) enum TurnWorkspaceRoots<'a> {
    /// Current client configuration, omitted when the server owns thread settings.
    Configured(&'a [AbsolutePathBuf]),
    /// Roots explicitly selected by the user and validated on the connected server.
    ProjectAttachment(&'a [AbsolutePathBuf]),
}
