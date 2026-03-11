use crate::file_reference_index;
use codex_core::config::types::UriBasedFileOpener;
use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use regex_lite::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;

static NON_WHITESPACE_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| match Regex::new(r"\S+") {
    Ok(regex) => regex,
    Err(error) => panic!("invalid non-whitespace token regex: {error}"),
});

static COLON_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(
        || match Regex::new(r":\d+(?::\d+)?(?:[-–]\d+(?::\d+)?)?$") {
            Ok(regex) => regex,
            Err(error) => panic!("invalid colon location suffix regex: {error}"),
        },
    );

static HASH_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"#L\d+(?:C\d+)?(?:-L\d+(?:C\d+)?)?$") {
        Ok(regex) => regex,
        Err(error) => panic!("invalid hash location suffix regex: {error}"),
    });

#[derive(Debug, Clone)]
pub(crate) struct FileLinkContext {
    cwd: PathBuf,
    file_opener: UriBasedFileOpener,
    markdown_link_targets: Option<Arc<HashMap<String, String>>>,
}

impl FileLinkContext {
    pub(crate) fn new(cwd: PathBuf, file_opener: UriBasedFileOpener) -> Self {
        Self {
            cwd,
            file_opener,
            markdown_link_targets: None,
        }
    }

    pub(crate) fn with_markdown_link_targets(
        cwd: PathBuf,
        file_opener: UriBasedFileOpener,
        markdown_link_targets: Arc<HashMap<String, String>>,
    ) -> Self {
        Self {
            cwd,
            file_opener,
            markdown_link_targets: Some(markdown_link_targets),
        }
    }

    pub(crate) fn file_opener(&self) -> UriBasedFileOpener {
        self.file_opener
    }
}

pub(crate) fn extract_markdown_link_targets(
    markdown_source: &str,
    cwd: &Path,
    file_opener: UriBasedFileOpener,
) -> Option<Arc<HashMap<String, String>>> {
    if file_opener == UriBasedFileOpener::None {
        return None;
    }

    #[derive(Debug)]
    struct PendingMarkdownLink {
        label: String,
        visible_suffix: Option<String>,
        uri: String,
    }

    let parser = Parser::new_ext(markdown_source, Options::ENABLE_STRIKETHROUGH);
    let mut pending_link: Option<PendingMarkdownLink> = None;
    let mut unique_targets: HashMap<String, Option<String>> = HashMap::new();

    for event in parser {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                let dest_url = dest_url.to_string();
                let Some(uri) = resolve_markdown_link_destination_uri(&dest_url, cwd, file_opener)
                else {
                    pending_link = None;
                    continue;
                };
                pending_link = Some(PendingMarkdownLink {
                    label: String::new(),
                    visible_suffix: markdown_link_visible_suffix(&dest_url),
                    uri,
                });
            }
            Event::End(TagEnd::Link) => {
                let Some(link) = pending_link.take() else {
                    continue;
                };
                if link.label.is_empty() {
                    continue;
                }

                let visible_label = if label_has_location_suffix(&link.label) {
                    link.label
                } else if let Some(visible_suffix) = link.visible_suffix {
                    format!("{}{visible_suffix}", link.label)
                } else {
                    link.label
                };

                match unique_targets.entry(visible_label) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(Some(link.uri));
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if entry.get().as_ref() != Some(&link.uri) {
                            entry.insert(None);
                        }
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(link) = pending_link.as_mut() {
                    link.label.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(link) = pending_link.as_mut() {
                    link.label.push(' ');
                }
            }
            Event::Rule
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::TaskListMarker(_)
            | Event::Start(_)
            | Event::End(_) => {}
        }
    }

    let targets = unique_targets
        .into_iter()
        .filter_map(|(label, uri)| uri.map(|uri| (label, uri)))
        .collect::<HashMap<_, _>>();

    (!targets.is_empty()).then(|| Arc::new(targets))
}

pub(crate) fn linkify_transcript_lines(
    lines: Vec<Line<'static>>,
    file_link_context: &FileLinkContext,
) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let alignment = line.alignment;
            let style = line.style;
            let mut linked_spans = Vec::with_capacity(line.spans.len());
            let mut spans = line.spans.into_iter().peekable();
            while let Some(span) = spans.next() {
                if span.style.fg != Some(Color::Cyan)
                    || span.content.trim().is_empty()
                    || span.content.contains("\x1B]8;;")
                {
                    linked_spans.push(span);
                    continue;
                }

                let style = span.style;
                let mut content = span.content.to_string();
                while let Some(next_span) = spans.next_if(|next_span| {
                    next_span.style == style
                        && next_span.style.fg == Some(Color::Cyan)
                        && !next_span.content.contains("\x1B]8;;")
                }) {
                    content.push_str(&next_span.content);
                }

                linked_spans.extend(linkify_cyan_content(&content, style, file_link_context));
            }

            Line {
                spans: linked_spans,
                style,
                alignment,
            }
        })
        .collect()
}

fn trim_token_punctuation(token: &str) -> (&str, usize, usize) {
    let mut start = 0usize;
    let mut end = token.len();

    while start < end {
        let Some(ch) = token[start..].chars().next() else {
            break;
        };
        if matches!(ch, '`' | '(' | '[' | '{' | '<' | '"' | '\'') {
            start += ch.len_utf8();
        } else {
            break;
        }
    }

    while end > start {
        let Some(ch) = token[..end].chars().next_back() else {
            break;
        };
        if matches!(
            ch,
            '`' | ')' | ']' | '}' | '>' | '"' | '\'' | '.' | ',' | ';' | '!' | '?'
        ) {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (&token[start..end], start, token.len().saturating_sub(end))
}

fn resolve_file_reference_uri(
    token: &str,
    cwd: &Path,
    file_opener: UriBasedFileOpener,
) -> Option<String> {
    let (raw_path, line, column) = split_file_reference_location(token)?;
    let resolved_path = resolve_file_reference_path(raw_path, cwd)?;
    let scheme = file_opener.get_scheme()?;
    let path = resolved_path.to_string_lossy().replace('\\', "/");
    let location = match line {
        Some(line) => match column {
            Some(column) => format!(":{line}:{column}"),
            None => format!(":{line}"),
        },
        None => String::new(),
    };
    Some(format!("{scheme}://file{path}{location}"))
}

fn resolve_markdown_link_destination_uri(
    dest_url: &str,
    cwd: &Path,
    file_opener: UriBasedFileOpener,
) -> Option<String> {
    let path = dest_url.strip_prefix("file://").unwrap_or(dest_url);
    resolve_file_reference_uri(path, cwd, file_opener)
}

fn split_file_reference_location(token: &str) -> Option<(&str, Option<u32>, Option<u32>)> {
    if let Some(location_match) = HASH_LOCATION_SUFFIX_RE.find(token)
        && location_match.end() == token.len()
    {
        let normalized =
            codex_utils_string::normalize_markdown_hash_location_suffix(location_match.as_str())?;
        let (line, column) = parse_colon_location_suffix(&normalized)?;
        let path = &token[..location_match.start()];
        return Some((path, Some(line), column));
    }

    if let Some(location_match) = COLON_LOCATION_SUFFIX_RE.find(token)
        && location_match.end() == token.len()
    {
        let suffix = location_match.as_str();
        let path = &token[..location_match.start()];
        let (line, column) = parse_colon_location_suffix(suffix)?;
        return Some((path, Some(line), column));
    }

    Some((token, None, None))
}

fn parse_colon_location_suffix(suffix: &str) -> Option<(u32, Option<u32>)> {
    let suffix = suffix.strip_prefix(':')?;
    let start = suffix
        .split_once(['-', '–'])
        .map(|(start, _)| start)
        .unwrap_or(suffix);
    let (line, column) = match start.split_once(':') {
        Some((line, column)) => (line, Some(column)),
        None => (start, None),
    };

    Some((
        line.parse().ok()?,
        column.and_then(|column| column.parse().ok()),
    ))
}

fn resolve_file_reference_path(raw_path: &str, cwd: &Path) -> Option<PathBuf> {
    if !looks_like_file_reference_path(raw_path) {
        return None;
    }

    let mut candidates = Vec::new();
    if let Some(rest) = raw_path.strip_prefix("~/") {
        if let Some(home_dir) = dirs::home_dir() {
            candidates.push(home_dir.join(rest));
        }
    } else {
        let path = PathBuf::from(raw_path);
        if path.is_absolute() {
            candidates.push(path);
        } else {
            if let Some(stripped) = raw_path
                .strip_prefix("a/")
                .or_else(|| raw_path.strip_prefix("b/"))
            {
                candidates.push(cwd.join(stripped));
            }
            candidates.push(cwd.join(raw_path));
        }
    }

    if let Some(path) = candidates.into_iter().find(|candidate| candidate.is_file()) {
        return Some(path);
    }

    if is_basename_file_reference(raw_path) {
        return file_reference_index::resolve_unique_basename(cwd, raw_path);
    }

    None
}

fn is_basename_file_reference(path: &str) -> bool {
    !path.starts_with("~/")
        && !path.starts_with("./")
        && !path.starts_with("../")
        && !path.starts_with('/')
        && !path.starts_with("a/")
        && !path.starts_with("b/")
        && !path.contains('/')
        && !path.contains('\\')
}

fn looks_like_file_reference_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }

    if path.starts_with("~/")
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with('/')
    {
        return true;
    }

    if path.contains('/') || path.contains('\\') {
        return true;
    }

    path.contains('.')
}

fn markdown_link_visible_suffix(dest_url: &str) -> Option<String> {
    if !is_local_path_like_link(dest_url) {
        return None;
    }

    dest_url
        .rsplit_once('#')
        .and_then(|(_, fragment)| {
            codex_utils_string::normalize_markdown_hash_location_suffix(&format!("#{fragment}"))
        })
        .or_else(|| {
            COLON_LOCATION_SUFFIX_RE
                .find(dest_url)
                .map(|location| location.as_str().to_string())
        })
}

fn label_has_location_suffix(label: &str) -> bool {
    split_file_reference_location(label).is_some_and(|(_, line, _)| line.is_some())
}

fn is_local_path_like_link(dest_url: &str) -> bool {
    dest_url.starts_with("file://")
        || dest_url.starts_with('/')
        || dest_url.starts_with("~/")
        || dest_url.starts_with("./")
        || dest_url.starts_with("../")
        || dest_url.starts_with("\\\\")
        || matches!(
            dest_url.as_bytes(),
            [drive, b':', separator, ..]
                if drive.is_ascii_alphabetic() && matches!(separator, b'/' | b'\\')
        )
}

fn linkify_cyan_content(
    content: &str,
    style: Style,
    file_link_context: &FileLinkContext,
) -> Vec<Span<'static>> {
    let mut linked_spans = Vec::new();
    let mut last = 0usize;

    for token_match in NON_WHITESPACE_TOKEN_RE.find_iter(content) {
        if token_match.start() > last {
            linked_spans.push(Span::styled(
                content[last..token_match.start()].to_string(),
                style,
            ));
        }

        let raw_token = token_match.as_str();
        let (candidate, trim_prefix, trim_suffix) = trim_token_punctuation(raw_token);
        let url = file_link_context
            .markdown_link_targets
            .as_ref()
            .and_then(|targets| targets.get(candidate).cloned())
            .or_else(|| {
                resolve_file_reference_uri(
                    candidate,
                    file_link_context.cwd.as_path(),
                    file_link_context.file_opener,
                )
            });
        let prefix_end = trim_prefix.min(raw_token.len());
        let suffix_start = raw_token.len().saturating_sub(trim_suffix);

        if prefix_end > 0 {
            linked_spans.push(Span::styled(raw_token[..prefix_end].to_string(), style));
        }

        let core_token = &raw_token[prefix_end..suffix_start];
        if let Some(url) = url {
            if let Some(wrapped) = osc8_wrapped_text(core_token, &url) {
                linked_spans.push(Span::styled(wrapped, style));
            } else {
                linked_spans.push(Span::styled(core_token.to_string(), style));
            }
        } else {
            linked_spans.push(Span::styled(core_token.to_string(), style));
        }

        if suffix_start < raw_token.len() {
            linked_spans.push(Span::styled(raw_token[suffix_start..].to_string(), style));
        }

        last = token_match.end();
    }

    if last < content.len() {
        linked_spans.push(Span::styled(content[last..].to_string(), style));
    }

    linked_spans
}

fn osc8_wrapped_text(text: &str, url: &str) -> Option<String> {
    let safe_url: String = url
        .chars()
        .filter(|&ch| ch != '\x1B' && ch != '\x07')
        .collect();
    if safe_url.is_empty() || text.trim().is_empty() {
        return None;
    }

    Some(format!("\x1B]8;;{safe_url}\x1B\\{text}\x1B]8;;\x1B\\"))
}
