use crate::file_reference_index;
use codex_config::types::UriBasedFileOpener;
use regex_lite::Regex;
use std::path::Path;
use std::path::PathBuf;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileReferenceSegment {
    pub(crate) text: String,
    pub(crate) href: Option<String>,
}

pub(crate) fn split_file_reference_segments(
    content: &str,
    cwd: &Path,
    file_opener: UriBasedFileOpener,
) -> Vec<FileReferenceSegment> {
    let mut segments = Vec::new();
    let mut last = 0usize;

    for token_match in NON_WHITESPACE_TOKEN_RE.find_iter(content) {
        if token_match.start() > last {
            segments.push(FileReferenceSegment {
                text: content[last..token_match.start()].to_string(),
                href: None,
            });
        }

        let raw_token = token_match.as_str();
        let (candidate, trim_prefix, trim_suffix) = trim_token_punctuation(raw_token);
        let prefix_end = trim_prefix.min(raw_token.len());
        let suffix_start = raw_token.len().saturating_sub(trim_suffix);

        if prefix_end > 0 {
            segments.push(FileReferenceSegment {
                text: raw_token[..prefix_end].to_string(),
                href: None,
            });
        }

        let core_token = &raw_token[prefix_end..suffix_start];
        segments.push(FileReferenceSegment {
            text: core_token.to_string(),
            href: resolve_file_reference_uri(candidate, cwd, file_opener),
        });

        if suffix_start < raw_token.len() {
            segments.push(FileReferenceSegment {
                text: raw_token[suffix_start..].to_string(),
                href: None,
            });
        }

        last = token_match.end();
    }

    if last < content.len() {
        segments.push(FileReferenceSegment {
            text: content[last..].to_string(),
            href: None,
        });
    }

    segments
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
            '`' | ')' | ']' | '}' | '>' | '"' | '\'' | '.' | ',' | ':' | ';' | '!' | '?'
        ) {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (&token[start..end], start, token.len().saturating_sub(end))
}

pub(crate) fn resolve_file_reference_uri(
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
