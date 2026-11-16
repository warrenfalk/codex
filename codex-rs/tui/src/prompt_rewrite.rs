//! Prompt-rewrite request preparation and immutable-region validation.
//!
//! The rewrite model receives a marker-encoded version of the draft. Rich composer elements and
//! literal payloads are replaced with opaque markers, then restored only if every marker survives
//! exactly once and in its original order. This makes the model useful for prose while keeping
//! attachments, mentions, pasted payloads, code, diffs, commands, logs, and quoted blocks exact.

use crate::bottom_pane::ComposerDraftSnapshot;
use codex_protocol::ThreadId;
use codex_protocol::user_input::TextElement;
use serde::Deserialize;
use serde_json::json;
use std::ops::Range;

const MARKER_STEM: &str = "__CODEX_PROMPT_REWRITE_PROTECTED";

#[derive(Clone, Debug)]
pub(crate) struct PromptRewriteRequest {
    pub(crate) parent_thread_id: ThreadId,
    pub(crate) original_draft: ComposerDraftSnapshot,
    protected_prompt: ProtectedPrompt,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RewrittenComposerDraft {
    pub(crate) text: String,
    pub(crate) text_elements: Vec<TextElement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptRewriteUnavailable {
    Empty,
    ShellCommand,
    SlashCommand,
    InvalidStructuredDraft,
}

#[derive(Clone, Debug)]
struct ProtectedPrompt {
    encoded_text: String,
    marker_prefix: String,
    regions: Vec<ProtectedRegion>,
}

#[derive(Clone, Debug)]
struct ProtectedRegion {
    marker: String,
    text: String,
    elements: Vec<ProtectedElement>,
}

#[derive(Clone, Debug)]
struct ProtectedElement {
    element: TextElement,
    relative_range: Range<usize>,
}

#[derive(Deserialize)]
struct RewriteResponse {
    rewritten_prompt: String,
}

impl PromptRewriteRequest {
    pub(crate) fn new(
        parent_thread_id: ThreadId,
        original_draft: ComposerDraftSnapshot,
        recognized_slash_command: bool,
    ) -> Result<Self, PromptRewriteUnavailable> {
        let trimmed = original_draft.text.trim_start();
        if trimmed.is_empty() {
            return Err(PromptRewriteUnavailable::Empty);
        }
        if trimmed.starts_with('!') {
            return Err(PromptRewriteUnavailable::ShellCommand);
        }
        if recognized_slash_command {
            return Err(PromptRewriteUnavailable::SlashCommand);
        }
        validate_structured_draft(&original_draft)?;
        let protected_prompt = ProtectedPrompt::new(&original_draft)?;
        Ok(Self {
            parent_thread_id,
            original_draft,
            protected_prompt,
        })
    }

    pub(crate) fn model_input(&self) -> String {
        format!(
            "Rewrite the following unsent user prompt. Return the rewritten prompt in the required JSON field.\n\n<draft>\n{}\n</draft>",
            self.protected_prompt.encoded_text
        )
    }

    pub(crate) fn output_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "rewritten_prompt": { "type": "string" }
            },
            "required": ["rewritten_prompt"],
            "additionalProperties": false
        })
    }

    pub(crate) fn restore_model_output(
        &self,
        model_output: &str,
    ) -> Result<RewrittenComposerDraft, String> {
        let response: RewriteResponse = serde_json::from_str(model_output.trim())
            .map_err(|err| format!("rewrite response was not valid JSON: {err}"))?;
        self.protected_prompt.restore(&response.rewritten_prompt)
    }
}

impl ProtectedPrompt {
    fn new(draft: &ComposerDraftSnapshot) -> Result<Self, PromptRewriteUnavailable> {
        let marker_prefix = marker_prefix_not_in(&draft.text);
        let mut ranges = draft
            .text_elements
            .iter()
            .map(|element| element.byte_range.start..element.byte_range.end)
            .collect::<Vec<_>>();
        ranges.extend(literal_ranges(&draft.text));
        let ranges = merge_ranges(ranges);

        let mut regions = Vec::with_capacity(ranges.len());
        let mut encoded_text = String::new();
        let mut previous_end = 0usize;
        for (index, range) in ranges.into_iter().enumerate() {
            if !valid_range(&draft.text, &range) || range.start < previous_end {
                return Err(PromptRewriteUnavailable::InvalidStructuredDraft);
            }
            encoded_text.push_str(&draft.text[previous_end..range.start]);
            let marker = format!("{marker_prefix}{index:04}__");
            encoded_text.push_str(&marker);
            let elements = draft
                .text_elements
                .iter()
                .filter(|element| {
                    element.byte_range.start >= range.start && element.byte_range.end <= range.end
                })
                .map(|element| ProtectedElement {
                    element: element.clone(),
                    relative_range: element.byte_range.start - range.start
                        ..element.byte_range.end - range.start,
                })
                .collect();
            regions.push(ProtectedRegion {
                marker,
                text: draft.text[range.clone()].to_string(),
                elements,
            });
            previous_end = range.end;
        }
        encoded_text.push_str(&draft.text[previous_end..]);

        Ok(Self {
            encoded_text,
            marker_prefix,
            regions,
        })
    }

    fn restore(&self, rewritten: &str) -> Result<RewrittenComposerDraft, String> {
        if rewritten.trim().is_empty() {
            return Err("rewrite response was empty".to_string());
        }
        if rewritten.match_indices(&self.marker_prefix).count() != self.regions.len() {
            return Err("rewrite changed protected markers".to_string());
        }

        let mut marker_positions = Vec::with_capacity(self.regions.len());
        for region in &self.regions {
            let positions = rewritten
                .match_indices(&region.marker)
                .map(|(position, _)| position)
                .collect::<Vec<_>>();
            let [position] = positions.as_slice() else {
                return Err("rewrite dropped or duplicated protected content".to_string());
            };
            marker_positions.push(*position);
        }
        if marker_positions.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("rewrite reordered protected content".to_string());
        }

        let mut text = String::new();
        let mut text_elements = Vec::new();
        let mut rewritten_cursor = 0usize;
        for (region, marker_position) in self.regions.iter().zip(marker_positions) {
            if marker_position < rewritten_cursor {
                return Err("rewrite produced overlapping protected markers".to_string());
            }
            text.push_str(&rewritten[rewritten_cursor..marker_position]);
            let region_start = text.len();
            text.push_str(&region.text);
            for protected in &region.elements {
                let start = region_start + protected.relative_range.start;
                let end = region_start + protected.relative_range.end;
                text_elements.push(protected.element.map_range(|_| (start..end).into()));
            }
            rewritten_cursor = marker_position + region.marker.len();
        }
        text.push_str(&rewritten[rewritten_cursor..]);

        Ok(RewrittenComposerDraft {
            text,
            text_elements,
        })
    }
}

fn validate_structured_draft(
    draft: &ComposerDraftSnapshot,
) -> Result<(), PromptRewriteUnavailable> {
    if draft.text_elements.iter().any(|element| {
        !valid_range(
            &draft.text,
            &(element.byte_range.start..element.byte_range.end),
        )
    }) {
        return Err(PromptRewriteUnavailable::InvalidStructuredDraft);
    }

    let element_texts = draft
        .text_elements
        .iter()
        .map(|element| &draft.text[element.byte_range.start..element.byte_range.end])
        .collect::<Vec<_>>();
    for token in draft
        .local_images
        .iter()
        .map(|image| image.placeholder.as_str())
        .chain(
            draft
                .pending_pastes
                .iter()
                .map(|(placeholder, _)| placeholder.as_str()),
        )
    {
        if !element_texts.contains(&token) {
            return Err(PromptRewriteUnavailable::InvalidStructuredDraft);
        }
    }
    for binding in &draft.mention_bindings {
        let token = format!("{}{}", binding.sigil, binding.mention);
        if !element_texts.contains(&token.as_str()) {
            return Err(PromptRewriteUnavailable::InvalidStructuredDraft);
        }
    }
    Ok(())
}

fn valid_range(text: &str, range: &Range<usize>) -> bool {
    range.start < range.end
        && range.end <= text.len()
        && text.is_char_boundary(range.start)
        && text.is_char_boundary(range.end)
}

fn marker_prefix_not_in(text: &str) -> String {
    let mut suffix = 0usize;
    loop {
        let candidate = format!("{MARKER_STEM}_{suffix}_");
        if !text.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn merge_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

#[derive(Clone, Debug)]
struct LineSpan<'a> {
    range: Range<usize>,
    content: &'a str,
}

fn line_spans(text: &str) -> Vec<LineSpan<'_>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for segment in text.split_inclusive('\n') {
        let end = start + segment.len();
        let content = segment.strip_suffix('\n').unwrap_or(segment);
        let content = content.strip_suffix('\r').unwrap_or(content);
        lines.push(LineSpan {
            range: start..end,
            content,
        });
        start = end;
    }
    if start < text.len() || text.is_empty() {
        lines.push(LineSpan {
            range: start..text.len(),
            content: &text[start..],
        });
    }
    lines
}

fn literal_ranges(text: &str) -> Vec<Range<usize>> {
    let lines = line_spans(text);
    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        if let Some((fence, width)) = fence_start(lines[index].content) {
            let start = lines[index].range.start;
            index += 1;
            while index < lines.len() {
                let closes = fence_start(lines[index].content).is_some_and(
                    |(candidate, candidate_width)| candidate == fence && candidate_width >= width,
                );
                index += 1;
                if closes {
                    break;
                }
            }
            ranges.push(start..lines[index.saturating_sub(1)].range.end);
            continue;
        }
        if is_triple_quote(lines[index].content) {
            let delimiter = lines[index].content.trim_start()[..3].to_string();
            let start = lines[index].range.start;
            index += 1;
            while index < lines.len() {
                let closes = lines[index].content.trim().ends_with(&delimiter);
                index += 1;
                if closes {
                    break;
                }
            }
            ranges.push(start..lines[index.saturating_sub(1)].range.end);
            continue;
        }
        if is_diff_start(&lines, index) {
            let start = lines[index].range.start;
            index += 1;
            while index < lines.len() && is_diff_line(lines[index].content) {
                index += 1;
            }
            ranges.push(start..lines[index.saturating_sub(1)].range.end);
            continue;
        }
        if is_log_line(lines[index].content) {
            let start = lines[index].range.start;
            index += 1;
            while index < lines.len()
                && (is_log_line(lines[index].content)
                    || lines[index].content.starts_with(char::is_whitespace))
            {
                index += 1;
            }
            ranges.push(start..lines[index.saturating_sub(1)].range.end);
            continue;
        }
        if is_command_or_quote(lines[index].content) {
            ranges.push(lines[index].range.clone());
        }
        index += 1;
    }
    ranges
}

fn fence_start(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let fence = trimmed.chars().next()?;
    if !matches!(fence, '`' | '~') {
        return None;
    }
    let width = trimmed
        .chars()
        .take_while(|candidate| *candidate == fence)
        .count();
    (width >= 3).then_some((fence, width))
}

fn is_triple_quote(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''")
}

fn is_diff_start(lines: &[LineSpan<'_>], index: usize) -> bool {
    let line = lines[index].content;
    line.starts_with("diff --git ")
        || line.starts_with("@@ ")
        || (line.starts_with("--- ")
            && lines
                .get(index + 1)
                .is_some_and(|next| next.content.starts_with("+++ ")))
}

fn is_diff_line(line: &str) -> bool {
    line.is_empty()
        || line.starts_with("diff --git ")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("@@ ")
        || line.starts_with('+')
        || line.starts_with('-')
        || line.starts_with(' ')
        || line.starts_with("\\ No newline at end of file")
}

fn is_log_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let level_prefix = [
        "TRACE ", "DEBUG ", "INFO ", "WARN ", "ERROR ", "[TRACE]", "[DEBUG]", "[INFO]", "[WARN]",
        "[ERROR]",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix));
    level_prefix || looks_like_timestamp(trimmed)
}

fn looks_like_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 11
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && matches!(bytes[10], b'T' | b' ')
}

fn is_command_or_quote(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("$ ")
        || trimmed.starts_with("% ")
        || trimmed.starts_with("> ")
        || trimmed.starts_with(">\t")
}

#[cfg(test)]
#[path = "prompt_rewrite_tests.rs"]
mod tests;
