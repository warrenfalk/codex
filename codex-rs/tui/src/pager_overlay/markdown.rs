//! A read-only Markdown pager with a persistent link to the original file.

use super::StaticOverlay;
use crate::history_cell::sanitize_user_text;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::keymap::PagerKeymap;
use crate::terminal_hyperlinks::HyperlinkRows;
use crate::terminal_hyperlinks::mark_hyperlink_cells;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use tokio::sync::oneshot;

const MAX_PREVIEW_BYTES: u64 = 256 * 1024;
const SOURCE_LABEL: &str = "Open source";
const OPEN_SOURCE_KEY: KeyBinding = key_hint::plain(KeyCode::Enter);

pub(crate) struct MarkdownOverlay {
    pager: StaticOverlay,
    load: Option<oneshot::Receiver<io::Result<String>>>,
    markdown: Option<String>,
    layout_width: Option<u16>,
    source_url: String,
    source_area: Rect,
    source_pressed: bool,
    open_external: Box<dyn Fn(String)>,
}

impl MarkdownOverlay {
    pub(crate) fn new(
        path: PathBuf,
        source_url: String,
        mut keymap: PagerKeymap,
        frames: FrameRequester,
        open_external: impl Fn(String) + 'static,
    ) -> Self {
        let close = key_hint::plain(KeyCode::Esc);
        if !keymap.close.contains(&close) {
            keymap.close.insert(/*index*/ 0, close);
        }
        let filename = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        let title = sanitize_user_text(filename).replace(['\n', '\t'], " ");
        let (send, load) = oneshot::channel();
        tokio::task::spawn_blocking(move || {
            if send.send(read_markdown(&path)).is_ok() {
                frames.schedule_frame();
            }
        });
        Self {
            pager: StaticOverlay::with_title(vec!["Loading Markdown…".dim().into()], title, keymap),
            load: Some(load),
            markdown: None,
            layout_width: None,
            source_url,
            source_area: Rect::default(),
            source_pressed: false,
            open_external: Box::new(open_external),
        }
    }

    fn finish_load(&mut self) {
        let Some(load) = &mut self.load else {
            return;
        };
        let result = match load.try_recv() {
            Ok(result) => result,
            Err(oneshot::error::TryRecvError::Empty) => return,
            Err(oneshot::error::TryRecvError::Closed) => {
                Err(io::Error::other("Markdown reader stopped"))
            }
        };
        self.load = None;
        let message = match result {
            Ok(markdown) if markdown.trim().is_empty() => "This file is empty.".to_string(),
            Ok(markdown) => {
                self.markdown = Some(sanitize_user_text(markdown.into()).into_owned());
                return;
            }
            Err(error) => format!("Could not preview Markdown.\n{error}"),
        };
        self.pager.view.renderables =
            vec![Box::new(Paragraph::new(message).wrap(Wrap { trim: false }))];
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.finish_load();
        self.source_area = Rect::default();
        if area.is_empty() {
            return;
        }
        Clear.render(area, buf);
        if area.width >= 7 && area.height >= 6 {
            if self.layout_width != Some(area.width)
                && let Some(markdown) = &self.markdown
            {
                let rows = crate::markdown::render_markdown_for_viewport(markdown, area.width);
                self.pager.view.renderables = vec![Box::new(HyperlinkRows::from(rows))];
                self.layout_width = Some(area.width);
            }
            self.pager.render(area, buf);
        }
        // The static pager reserves one blank row below the navigation hints.
        let row = Rect::new(area.x, area.bottom() - 1, area.width, /*height*/ 1);
        Line::from(vec![
            SOURCE_LABEL.cyan().underlined(),
            " (".dim(),
            OPEN_SOURCE_KEY.into(),
            ")".dim(),
        ])
        .render(row, buf);
        self.source_area = Rect::new(
            row.x,
            row.y,
            row.width.min(SOURCE_LABEL.len() as u16),
            /*height*/ 1,
        );
        mark_hyperlink_cells(buf, self.source_area, &self.source_url);
    }

    pub(crate) fn source_at(&self, mouse: MouseEvent) -> bool {
        (mouse.modifiers.is_empty()
            || mouse
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER))
            && self.source_area.contains((mouse.column, mouse.row).into())
    }

    pub(crate) fn handle_event(&mut self, tui: &mut Tui, event: TuiEvent) -> io::Result<()> {
        match event {
            TuiEvent::Draw | TuiEvent::Resume | TuiEvent::Resize(_) | TuiEvent::FocusGained => {
                tui.draw(u16::MAX, |frame| self.render(frame.area(), frame.buffer))?;
            }
            TuiEvent::Key(key)
                if key.kind == KeyEventKind::Press && OPEN_SOURCE_KEY.is_press(key) =>
            {
                (self.open_external)(self.source_url.clone());
            }
            TuiEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.source_pressed = self.source_at(mouse);
                }
                MouseEventKind::Up(MouseButton::Left)
                    if std::mem::take(&mut self.source_pressed) && self.source_at(mouse) =>
                {
                    (self.open_external)(self.source_url.clone());
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    self.source_pressed = false;
                    let offset = &mut self.pager.view.scroll_offset;
                    *offset = if mouse.kind == MouseEventKind::ScrollUp {
                        offset.saturating_sub(1)
                    } else {
                        offset.saturating_add(1)
                    };
                    tui.frame_requester().schedule_frame();
                }
                MouseEventKind::Drag(MouseButton::Left) => self.source_pressed = false,
                _ => {}
            },
            TuiEvent::FocusLost => self.source_pressed = false,
            event => self.pager.handle_event(tui, event)?,
        }
        Ok(())
    }

    pub(crate) fn is_done(&self) -> bool {
        self.pager.is_done()
    }
}

fn read_markdown(path: &Path) -> io::Result<String> {
    let reject_non_file = || io::Error::other("The path does not point to a regular file.");
    if !path.metadata()?.is_file() {
        return Err(reject_non_file());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A replaced file must not be able to block the reader by becoming a named pipe.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(reject_non_file());
    }
    let mut bytes = Vec::new();
    file.take(MAX_PREVIEW_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PREVIEW_BYTES {
        return Err(io::Error::other(format!(
            "Markdown previews are limited to {} KiB.",
            MAX_PREVIEW_BYTES / 1024
        )));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
