//! Shared history-cell building blocks reused across transcript concerns.
//! Wrapped prefixes remain display-only while annotated rows retain their logical source.

use super::*;

#[derive(Debug)]
pub(crate) struct PlainHistoryCell {
    pub(super) lines: Vec<Line<'static>>,
    visibility_kind: HistoryVisibilityKind,
}

impl PlainHistoryCell {
    pub(crate) fn new(lines: Vec<Line<'static>>) -> Self {
        Self::new_with_visibility_kind(lines, HistoryVisibilityKind::Normal)
    }

    pub(crate) fn new_with_visibility_kind(
        lines: Vec<Line<'static>>,
        visibility_kind: HistoryVisibilityKind,
    ) -> Self {
        Self {
            lines,
            visibility_kind,
        }
    }
}

impl HistoryCell for PlainHistoryCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines.clone()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.lines.clone())
    }

    fn history_visibility_kind(&self) -> HistoryVisibilityKind {
        self.visibility_kind
    }
}

#[derive(Debug)]
pub(crate) struct WebHyperlinkHistoryCell {
    lines: Vec<HyperlinkLine>,
    visibility_kind: HistoryVisibilityKind,
}

impl WebHyperlinkHistoryCell {
    #[cfg(test)]
    pub(crate) fn new(lines: Vec<Line<'static>>) -> Self {
        Self::new_with_visibility_kind(lines, HistoryVisibilityKind::Normal)
    }

    pub(crate) fn new_hyperlink_lines(
        lines: Vec<HyperlinkLine>,
        visibility_kind: HistoryVisibilityKind,
    ) -> Self {
        Self {
            lines,
            visibility_kind,
        }
    }

    pub(crate) fn new_with_visibility_kind(
        lines: Vec<Line<'static>>,
        visibility_kind: HistoryVisibilityKind,
    ) -> Self {
        Self {
            lines: crate::terminal_hyperlinks::annotate_web_urls(lines),
            visibility_kind,
        }
    }
}

impl HistoryCell for WebHyperlinkHistoryCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines.iter().map(|line| line.line.clone()).collect()
    }

    fn display_hyperlink_lines(&self, _width: u16) -> Vec<HyperlinkLine> {
        self.lines.clone()
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.display_hyperlink_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.lines.iter().map(|line| line.line.clone()))
    }

    fn history_visibility_kind(&self) -> HistoryVisibilityKind {
        self.visibility_kind
    }
}
#[derive(Debug)]
pub(crate) struct PrefixedWrappedHistoryCell {
    text: Text<'static>,
    initial_prefix: Line<'static>,
    subsequent_prefix: Line<'static>,
    visibility_kind: HistoryVisibilityKind,
}

impl PrefixedWrappedHistoryCell {
    pub(crate) fn new(
        text: impl Into<Text<'static>>,
        initial_prefix: impl Into<Line<'static>>,
        subsequent_prefix: impl Into<Line<'static>>,
    ) -> Self {
        Self::new_with_visibility_kind(
            text,
            initial_prefix,
            subsequent_prefix,
            HistoryVisibilityKind::Normal,
        )
    }

    pub(crate) fn new_with_visibility_kind(
        text: impl Into<Text<'static>>,
        initial_prefix: impl Into<Line<'static>>,
        subsequent_prefix: impl Into<Line<'static>>,
        visibility_kind: HistoryVisibilityKind,
    ) -> Self {
        Self {
            text: text.into(),
            initial_prefix: initial_prefix.into(),
            subsequent_prefix: subsequent_prefix.into(),
            visibility_kind,
        }
    }
}

impl HistoryCell for PrefixedWrappedHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        visible_lines(self.display_hyperlink_lines(width))
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        if width == 0 {
            return Vec::new();
        }
        let opts = RtOptions::new(usize::from(width))
            .initial_indent(self.initial_prefix.clone())
            .subsequent_indent(self.subsequent_prefix.clone());
        crate::terminal_hyperlinks::adaptive_wrap_hyperlink_lines(
            &plain_hyperlink_lines(self.text.lines.clone()),
            opts,
        )
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.display_hyperlink_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.text.clone().lines)
    }

    fn history_visibility_kind(&self) -> HistoryVisibilityKind {
        self.visibility_kind
    }
}
#[derive(Debug)]
pub(crate) struct CompositeHistoryCell {
    pub(super) parts: Vec<Box<dyn HistoryCell>>,
    visibility_kind: HistoryVisibilityKind,
}

impl CompositeHistoryCell {
    pub(crate) fn new(parts: Vec<Box<dyn HistoryCell>>) -> Self {
        Self::new_with_visibility_kind(parts, HistoryVisibilityKind::Normal)
    }

    pub(crate) fn new_with_visibility_kind(
        parts: Vec<Box<dyn HistoryCell>>,
        visibility_kind: HistoryVisibilityKind,
    ) -> Self {
        Self {
            parts,
            visibility_kind,
        }
    }
}

impl HistoryCell for CompositeHistoryCell {
    fn warning_entries(&self) -> Vec<WarningEntry> {
        self.parts
            .iter()
            .flat_map(|part| part.warning_entries())
            .collect()
    }

    fn live_raw_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for part in &self.parts {
            let part = part.live_raw_lines();
            if !part.is_empty() {
                if !lines.is_empty() {
                    lines.push(Line::default());
                }
                lines.extend(part);
            }
        }
        lines
    }

    fn warning_keys(&self) -> Vec<WarningKey<'_>> {
        self.parts
            .iter()
            .flat_map(|part| part.warning_keys())
            .collect()
    }

    fn compact_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        let mut out = Vec::new();
        for part in &self.parts {
            let lines = part.compact_hyperlink_lines(width);
            if !lines.is_empty() {
                if !out.is_empty() {
                    out.push(HyperlinkLine::from(""));
                }
                out.extend(lines);
            }
        }
        out
    }

    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();
        let mut first = true;
        for part in &self.parts {
            let mut lines = part.display_lines(width);
            if !lines.is_empty() {
                if !first {
                    out.push(Line::from(""));
                }
                out.append(&mut lines);
                first = false;
            }
        }
        out
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        let mut out = Vec::new();
        let mut first = true;
        for part in &self.parts {
            let mut lines = part.display_hyperlink_lines(width);
            if !lines.is_empty() {
                if !first {
                    out.push(HyperlinkLine::from(""));
                }
                out.append(&mut lines);
                first = false;
            }
        }
        out
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        let mut out = Vec::new();
        let mut first = true;
        for part in &self.parts {
            let mut lines = part.transcript_hyperlink_lines(width);
            if !lines.is_empty() {
                if !first {
                    out.push(HyperlinkLine::from(""));
                }
                out.append(&mut lines);
                first = false;
            }
        }
        out
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();
        let mut first = true;
        for part in &self.parts {
            let mut lines = part.raw_lines();
            if !lines.is_empty() {
                if !first {
                    out.push(Line::from(""));
                }
                out.append(&mut lines);
                first = false;
            }
        }
        out
    }

    fn has_stable_transcript_height(&self) -> bool {
        false
    }

    fn history_visibility_kind(&self) -> HistoryVisibilityKind {
        self.visibility_kind
    }
}

#[cfg(test)]
#[path = "base_tests.rs"]
mod tests;
