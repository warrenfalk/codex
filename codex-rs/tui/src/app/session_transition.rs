//! Session transitions deferred until the event dispatcher has returned.

use super::App;
use super::AppRunControl;
use crate::app_event::SideConversationCloseChoice;
use crate::app_server_session::AppServerSession;
use crate::chatwidget::UserMessage;
use crate::tui::Tui;
use codex_app_server_protocol::ThreadStartSource;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::eyre::Result;

pub(super) enum PendingSessionTransition {
    NewSession {
        name: Option<String>,
    },
    ClearSession {
        user_message: Option<UserMessage>,
        name: Option<String>,
    },
    NewAgentsOverviewSession {
        cwd: Option<AbsolutePathBuf>,
    },
    StartSide {
        parent_thread_id: ThreadId,
        user_message: Option<UserMessage>,
    },
    CloseSide(SideConversationCloseChoice),
}

impl PendingSessionTransition {
    pub(super) async fn run(
        self,
        app: &mut App,
        tui: &mut Tui,
        app_server: &mut AppServerSession,
    ) -> Result<AppRunControl> {
        match self {
            Self::NewSession { name } => {
                Box::pin(app.start_fresh_session(
                    tui, app_server, /*session_start_source*/ None,
                    /*initial_user_message*/ None, name,
                ))
                .await;
                if app.chat_widget.has_misalignment_policy_violation() {
                    app.chat_widget.show_misalignment_policy_precaution();
                }
                Ok(AppRunControl::Continue)
            }
            Self::ClearSession { user_message, name } => {
                Box::pin(app.start_fresh_session(
                    tui,
                    app_server,
                    Some(ThreadStartSource::Clear),
                    user_message,
                    name,
                ))
                .await;
                Ok(AppRunControl::Continue)
            }
            Self::NewAgentsOverviewSession { cwd } => {
                Box::pin(app.new_agents_overview_session(tui, app_server, cwd)).await
            }
            Self::StartSide {
                parent_thread_id,
                user_message,
            } => {
                Box::pin(app.handle_start_side(tui, app_server, parent_thread_id, user_message))
                    .await
            }
            Self::CloseSide(choice) => {
                Box::pin(app.handle_side_conversation_close_choice(tui, app_server, choice))
                    .await
                    .map(|()| AppRunControl::Continue)
            }
        }
    }
}
