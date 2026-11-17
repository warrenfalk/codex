//! A persistent control notice that stays visible during turns and approval dialogs.
use super::*;
use codex_app_server_protocol::ThreadControlRequestStatus;
use codex_app_server_protocol::ThreadControlState;

impl BottomPane {
    pub(crate) fn set_thread_control_state(&mut self, state: Option<ThreadControlState>) {
        if self.thread_control != state {
            self.thread_control = state;
            self.request_redraw();
        }
    }

    pub(super) fn renderable_for_views<'a>(
        &'a self,
        options: ComposerRenderOptions<'a>,
        views: &'a [Box<dyn BottomPaneView>],
    ) -> RenderableItem<'a> {
        let Some(state) = self.thread_control.as_ref().filter(|state| {
            state.locked
                || state.grant.is_some()
                || state
                    .request
                    .as_ref()
                    .is_some_and(|request| request.status == ThreadControlRequestStatus::Pending)
        }) else {
            return self.content_renderable_for_views(options, views);
        };
        if state
            .request
            .as_ref()
            .is_some_and(|r| r.auto_grant_at.is_some())
        {
            self.frame_requester
                .schedule_frame_in(Duration::from_secs(/*secs*/ 1));
        }
        let mut flex = FlexRenderable::new();
        flex.push(
            /*flex*/ 0,
            RenderableItem::Owned(Box::new(ControlNotice::new(
                state,
                chrono::Utc::now().timestamp(),
            ))),
        );
        flex.push(
            /*flex*/ 0,
            self.content_renderable_for_views(options, views),
        );
        RenderableItem::Owned(Box::new(flex))
    }
}

struct ControlNotice(Vec<Line<'static>>);

impl ControlNotice {
    fn new(state: &ThreadControlState, now: i64) -> Self {
        let mut lines = Vec::new();
        if let Some(request) = state
            .request
            .as_ref()
            .filter(|r| r.status == ThreadControlRequestStatus::Pending)
        {
            let timeout = request.auto_grant_at.map_or_else(
                || "explicit approval required".to_owned(),
                |deadline| format!("auto-grants in {}s", (deadline - now).max(0)),
            );
            lines.push(
                format!("{} requests control · {timeout}", request.controller_id)
                    .magenta()
                    .bold()
                    .into(),
            );
            lines.extend(
                crate::text_formatting::truncate_text(&request.reason, /*max_graphemes*/ 160)
                    .lines()
                    .take(3)
                    .map(|line| Line::from(line.to_owned())),
            );
            lines.push(
                "/control approve · /control deny · /control lock"
                    .cyan()
                    .into(),
            );
        } else if let Some(grant) = &state.grant {
            lines.push(
                format!(
                    "Controlled by {} · /control revoke · /control lock",
                    grant.controller_id
                )
                .magenta()
                .into(),
            );
        }
        if state.locked {
            lines.push("External control locked · /control unlock".magenta().into());
        }
        Self(lines)
    }
}

impl Renderable for ControlNotice {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        use ratatui::widgets::Widget;
        let lines = crate::wrapping::word_wrap_lines(&self.0, area.width as usize);
        Widget::render(Paragraph::new(lines), area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        crate::wrapping::word_wrap_lines(&self.0, width as usize).len() as u16
    }
}

#[cfg(test)]
#[path = "thread_control_tests.rs"]
mod tests;
