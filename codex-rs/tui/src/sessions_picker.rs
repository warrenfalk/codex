use std::path::Path;

use crate::key_hint;
use crate::kitty;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::remote_sessions::RemoteSessionsClient;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::prelude::Widget;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::WidgetRef;
use tokio_stream::StreamExt;
use unicode_width::UnicodeWidthStr;

pub(crate) async fn run_sessions_picker(
    tui: &mut Tui,
    sessions: &mut RemoteSessionsClient,
) -> Result<()> {
    let mut screen = SessionsPickerScreen::new(tui.frame_requester());
    screen.reload_all(sessions).await?;

    tui.draw(u16::MAX, |frame| {
        frame.render_widget_ref(&screen, frame.area());
    })?;

    let events = tui.event_stream();
    tokio::pin!(events);

    while !screen.is_done() {
        tokio::select! {
            Some(event) = events.next() => {
                match event {
                    TuiEvent::Key(key_event) => screen.handle_key(key_event),
                    TuiEvent::Paste(_) => {}
                    TuiEvent::Draw => {
                        tui.draw(u16::MAX, |frame| {
                            frame.render_widget_ref(&screen, frame.area());
                        })?;
                    }
                }
            }
            event = sessions.next_notification() => {
                let Some(event) = event? else {
                    screen.set_footer_message("app-server disconnected".to_string());
                    screen.close();
                    continue;
                };
                screen.handle_server_notification(sessions, event).await?;
            }
        }

        if let Some(thread_id) = screen.take_focus_request() {
            match kitty::focus_thread(&thread_id).await {
                Ok(()) => screen.close(),
                Err(err) => {
                    screen.set_footer_message(format!("Failed to focus {thread_id}: {err}"));
                }
            }
        }
    }

    Ok(())
}

struct SessionsPickerScreen {
    request_frame: FrameRequester,
    entries: Vec<SessionEntry>,
    selected_thread_id: Option<String>,
    pending_focus_thread_id: Option<String>,
    should_close: bool,
    footer_message: Option<String>,
}

#[derive(Clone)]
struct SessionEntry {
    thread: Thread,
}

impl SessionsPickerScreen {
    fn new(request_frame: FrameRequester) -> Self {
        Self {
            request_frame,
            entries: Vec::new(),
            selected_thread_id: None,
            pending_focus_thread_id: None,
            should_close: false,
            footer_message: None,
        }
    }

    fn is_done(&self) -> bool {
        self.should_close
    }

    fn close(&mut self) {
        self.should_close = true;
        self.request_frame.schedule_frame();
    }

    fn set_footer_message(&mut self, message: String) {
        self.footer_message = Some(message);
        self.request_frame.schedule_frame();
    }

    fn selected_index(&self) -> usize {
        self.selected_thread_id
            .as_ref()
            .and_then(|thread_id| {
                self.entries
                    .iter()
                    .position(|entry| entry.thread.id == *thread_id)
            })
            .unwrap_or(0)
    }

    fn current_thread_id(&self) -> Option<&str> {
        let index = self.selected_index();
        self.entries
            .get(index)
            .map(|entry| entry.thread.id.as_str())
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }

        let current = self.selected_index() as isize;
        let max_index = self.entries.len().saturating_sub(1) as isize;
        let next = (current + delta).clamp(0, max_index) as usize;
        self.selected_thread_id = Some(self.entries[next].thread.id.clone());
        self.request_frame.schedule_frame();
    }

    fn activate_selected(&mut self) {
        if let Some(thread_id) = self.current_thread_id() {
            self.pending_focus_thread_id = Some(thread_id.to_string());
            self.request_frame.schedule_frame();
        }
    }

    fn take_focus_request(&mut self) -> Option<String> {
        self.pending_focus_thread_id.take()
    }

    fn handle_key(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }
        if key_event.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            self.close();
            return;
        }

        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter => self.activate_selected(),
            KeyCode::Esc | KeyCode::Char('q') => self.close(),
            _ => {}
        }
    }

    async fn reload_all(&mut self, sessions: &mut RemoteSessionsClient) -> Result<()> {
        let thread_ids = sessions.loaded_thread_ids().await?;
        let mut entries = Vec::with_capacity(thread_ids.len());
        for thread_id in thread_ids {
            if let Some(thread) = self.read_thread(sessions, &thread_id).await? {
                entries.push(SessionEntry { thread });
            }
        }
        self.entries = entries;
        self.sort_entries();
        self.ensure_valid_selection();
        self.footer_message = None;
        self.request_frame.schedule_frame();
        Ok(())
    }

    async fn refresh_thread(
        &mut self,
        sessions: &mut RemoteSessionsClient,
        thread_id: &str,
    ) -> Result<()> {
        let Some(thread) = self.read_thread(sessions, thread_id).await? else {
            self.remove_thread(thread_id);
            return Ok(());
        };
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.thread.id == thread.id)
        {
            existing.thread = thread;
        } else {
            self.entries.push(SessionEntry { thread });
        }
        self.sort_entries();
        self.ensure_valid_selection();
        self.footer_message = None;
        self.request_frame.schedule_frame();
        Ok(())
    }

    fn remove_thread(&mut self, thread_id: &str) {
        self.entries.retain(|entry| entry.thread.id != thread_id);
        self.ensure_valid_selection();
        self.request_frame.schedule_frame();
    }

    async fn handle_server_notification(
        &mut self,
        sessions: &mut RemoteSessionsClient,
        event: ServerNotification,
    ) -> Result<()> {
        match event {
            ServerNotification::ThreadStarted(notification) => {
                self.refresh_thread(sessions, &notification.thread.id)
                    .await?;
            }
            ServerNotification::ThreadStatusChanged(notification) => {
                self.refresh_thread(sessions, &notification.thread_id)
                    .await?;
            }
            ServerNotification::ThreadClosed(notification) => {
                self.remove_thread(&notification.thread_id);
            }
            _ => {}
        }
        Ok(())
    }

    async fn read_thread(
        &mut self,
        sessions: &mut RemoteSessionsClient,
        thread_id: &str,
    ) -> Result<Option<Thread>> {
        match sessions.read_thread(thread_id).await {
            Ok(thread) => Ok(Some(thread)),
            Err(err) => {
                if err
                    .to_string()
                    .contains("includeTurns is unavailable before first user message")
                {
                    return Ok(None);
                }
                self.footer_message = Some(format!("Failed to read thread {thread_id}: {err}"));
                Ok(None)
            }
        }
    }

    fn sort_entries(&mut self) {
        self.entries.sort_by(|left, right| {
            right
                .thread
                .updated_at
                .cmp(&left.thread.updated_at)
                .then_with(|| {
                    session_directory_name(&left.thread.cwd)
                        .cmp(&session_directory_name(&right.thread.cwd))
                })
                .then_with(|| left.thread.id.cmp(&right.thread.id))
        });
    }

    fn ensure_valid_selection(&mut self) {
        match self.selected_thread_id.as_ref() {
            Some(thread_id)
                if self
                    .entries
                    .iter()
                    .any(|entry| entry.thread.id == *thread_id) => {}
            _ => {
                self.selected_thread_id = self.entries.first().map(|entry| entry.thread.id.clone());
            }
        }
    }
}

impl WidgetRef for &SessionsPickerScreen {
    fn render_ref(&self, area: ratatui::layout::Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        if area.height == 0 || area.width == 0 {
            return;
        }

        let title = Line::from(vec!["Active Sessions".bold()]);
        Paragraph::new(title).render(
            ratatui::layout::Rect::new(area.x, area.y, area.width, 1),
            buf,
        );

        let help = Line::from(vec![
            "Press ".dim(),
            key_hint::plain(KeyCode::Enter).into(),
            " to focus, ".dim(),
            key_hint::plain(KeyCode::Esc).into(),
            " to close".dim(),
        ]);
        if area.height > 1 {
            Paragraph::new(help).render(
                ratatui::layout::Rect::new(area.x, area.y + 1, area.width, 1),
                buf,
            );
        }

        let list_y = area.y.saturating_add(3);
        let footer_y = area.y + area.height.saturating_sub(1);
        if list_y >= footer_y {
            return;
        }
        let list_height = footer_y.saturating_sub(list_y);
        let list_area = ratatui::layout::Rect::new(area.x, list_y, area.width, list_height);

        if self.entries.is_empty() {
            Paragraph::new("No active sessions.".dim()).render(list_area, buf);
        } else {
            let dir_width = self
                .entries
                .iter()
                .map(|entry| {
                    UnicodeWidthStr::width(session_directory_name(&entry.thread.cwd).as_str())
                })
                .max()
                .unwrap_or(0)
                .clamp(8, 24);
            let branch_width = self
                .entries
                .iter()
                .map(|entry| UnicodeWidthStr::width(session_branch_label(&entry.thread).as_str()))
                .max()
                .unwrap_or(0)
                .clamp(1, 20);
            let selected = self.selected_index();
            let capacity = list_area.height as usize;
            let start = if capacity == 0 {
                0
            } else {
                selected.saturating_sub(capacity.saturating_sub(1))
            };
            let end = self.entries.len().min(start.saturating_add(capacity));

            for (offset, entry) in self.entries[start..end].iter().enumerate() {
                let is_selected = start + offset == selected;
                let marker = if is_selected {
                    "> ".cyan().bold()
                } else {
                    "  ".into()
                };
                let directory = session_directory_name(&entry.thread.cwd);
                let branch = session_branch_label(&entry.thread);
                let state = session_final_field(&entry.thread);
                let branch_span = if branch == "-" {
                    Span::from(format!("{branch:<branch_width$}")).dim()
                } else {
                    Span::from(format!("{branch:<branch_width$}")).cyan()
                };
                let line = truncate_line_with_ellipsis_if_overflow(
                    Line::from(vec![
                        marker,
                        Span::from(format!("{directory:<dir_width$}")).bold(),
                        " ".into(),
                        branch_span,
                        " ".dim(),
                        Span::from(state),
                    ]),
                    list_area.width as usize,
                );
                Paragraph::new(line).render(
                    ratatui::layout::Rect::new(
                        list_area.x,
                        list_area.y + offset as u16,
                        list_area.width,
                        1,
                    ),
                    buf,
                );
            }
        }

        let footer = self
            .footer_message
            .clone()
            .unwrap_or_else(|| format!("{} loaded session(s)", self.entries.len()));
        Paragraph::new(Line::from(footer).dim()).render(
            ratatui::layout::Rect::new(area.x, footer_y, area.width, 1),
            buf,
        );
    }
}

fn session_directory_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn session_branch_label(thread: &Thread) -> String {
    thread
        .git_info
        .as_ref()
        .and_then(|git_info| git_info.branch.clone())
        .unwrap_or_else(|| "-".to_string())
}

fn session_final_field(thread: &Thread) -> String {
    match &thread.status {
        ThreadStatus::Active { active_flags } if active_flags.is_empty() => "working".to_string(),
        ThreadStatus::SystemError => "system error".to_string(),
        ThreadStatus::Active { .. } | ThreadStatus::Idle | ThreadStatus::NotLoaded => {
            latest_assistant_snippet(thread).unwrap_or_else(|| "waiting".to_string())
        }
    }
}

fn latest_assistant_snippet(thread: &Thread) -> Option<String> {
    thread.turns.iter().rev().find_map(|turn| {
        turn.items.iter().rev().find_map(|item| match item {
            ThreadItem::AgentMessage { text, .. } => {
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_backend::VT100Backend;
    use codex_app_server_protocol::GitInfo;
    use codex_app_server_protocol::Turn;
    use codex_app_server_protocol::TurnStatus;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;

    fn sample_thread(
        id: &str,
        cwd: &str,
        branch: Option<&str>,
        status: ThreadStatus,
        assistant_text: Option<&str>,
        updated_at: i64,
    ) -> Thread {
        Thread {
            id: id.to_string(),
            preview: String::new(),
            ephemeral: false,
            model_provider: "openai".to_string(),
            created_at: 0,
            updated_at,
            status,
            path: None,
            cwd: cwd.into(),
            cli_version: "0.0.0".to_string(),
            source: codex_app_server_protocol::SessionSource::Cli,
            agent_nickname: None,
            agent_role: None,
            git_info: Some(GitInfo {
                sha: None,
                branch: branch.map(ToOwned::to_owned),
                origin_url: None,
            }),
            name: None,
            turns: assistant_text
                .map(|text| {
                    vec![Turn {
                        id: "turn-1".to_string(),
                        items: vec![ThreadItem::AgentMessage {
                            id: "msg-1".to_string(),
                            text: text.to_string(),
                            phase: None,
                            memory_citation: None,
                        }],
                        status: TurnStatus::Completed,
                        error: None,
                    }]
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn sessions_picker_snapshot() {
        let screen = SessionsPickerScreen {
            request_frame: FrameRequester::test_dummy(),
            entries: vec![
                SessionEntry {
                    thread: sample_thread(
                        "thread-1",
                        "/home/warren/source/codex-cli/codex",
                        Some("feature/sessions"),
                        ThreadStatus::Active {
                            active_flags: Vec::new(),
                        },
                        Some("Working reply"),
                        20,
                    ),
                },
                SessionEntry {
                    thread: sample_thread(
                        "thread-2",
                        "/home/warren/insurance",
                        Some("main"),
                        ThreadStatus::Idle,
                        Some("Need you to decide between option A and option B."),
                        10,
                    ),
                },
            ],
            selected_thread_id: Some("thread-1".to_string()),
            pending_focus_thread_id: None,
            should_close: false,
            footer_message: None,
        };
        let mut terminal = Terminal::new(VT100Backend::new(80, 8)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget_ref(&screen, frame.area()))
            .expect("render sessions picker");
        assert_snapshot!("sessions_picker", terminal.backend());
    }

    #[test]
    fn latest_assistant_snippet_prefers_latest_turn_message() {
        let mut thread = sample_thread(
            "thread-1",
            "/tmp/project",
            Some("main"),
            ThreadStatus::Idle,
            Some("first answer"),
            1,
        );
        thread.turns.push(Turn {
            id: "turn-2".to_string(),
            items: vec![ThreadItem::AgentMessage {
                id: "msg-2".to_string(),
                text: "second answer".to_string(),
                phase: None,
                memory_citation: None,
            }],
            status: TurnStatus::Completed,
            error: None,
        });

        assert_eq!(
            latest_assistant_snippet(&thread).as_deref(),
            Some("second answer")
        );
    }
}
