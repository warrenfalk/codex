//! Preserve the exact directory behind a user-selected project in model input and replay.
//!
//! Only bound atomic selections are linked. This is the user's selected reference, with no
//! additional instructions or project contents added to the prompt.

use codex_app_server_protocol::UserInput;
use codex_protocol::user_input::TextElement;

use crate::bottom_pane::MentionBinding;

pub(crate) fn encode(items: &mut [UserInput], bindings: &[MentionBinding]) {
    if !bindings
        .iter()
        .any(|binding| crate::project_mentions::project_path(&binding.path).is_some())
    {
        return;
    }
    let mut bindings = bindings.iter().peekable();
    for item in items {
        let UserInput::Text {
            text,
            text_elements,
        } = item
        else {
            continue;
        };
        let original = text.clone();
        let mut encoded = String::new();
        let mut offset = 0;
        for element in text_elements {
            let range = element.byte_range.clone();
            let Some(value) = original.get(range.start..range.end) else {
                continue;
            };
            let Some(prefix) = original.get(offset..range.start) else {
                continue;
            };
            let label = element.placeholder().unwrap_or(value);
            encoded.push_str(prefix);
            let start = encoded.len();
            if let Some(binding) = bindings.next_if(|binding| {
                label.strip_prefix(binding.sigil) == Some(binding.mention.as_str())
            }) && crate::project_mentions::project_path(&binding.path).is_some()
            {
                encoded.push_str(&format!("[@{}]({})", binding.mention, binding.path));
            } else {
                encoded.push_str(value);
            }
            element.byte_range = codex_app_server_protocol::ByteRange {
                start,
                end: encoded.len(),
            };
            offset = range.end;
        }
        encoded.push_str(&original[offset..]);
        *text = encoded;
    }
}

pub(crate) fn decode(text: &str, elements: Vec<TextElement>) -> (String, Vec<TextElement>) {
    if !text.contains("project://") {
        return (text.to_string(), elements);
    }
    let mut decoded = String::new();
    let mut decoded_elements = Vec::new();
    let mut offset = 0;
    for element in elements {
        let range = element.byte_range;
        let Some(prefix) = text.get(offset..range.start) else {
            continue;
        };
        let Some(value) = text.get(range.start..range.end) else {
            continue;
        };
        decoded.push_str(prefix);
        let start = decoded.len();
        if let Some((name, path, end)) =
            crate::mention_codec::parse_linked_tool_mention(text, range.start, '@')
            && end == range.end
            && crate::project_mentions::project_path(path).is_some()
            && element
                .placeholder(text)
                .and_then(|label| label.strip_prefix('@'))
                == Some(name)
        {
            decoded.push('@');
            decoded.push_str(name);
        } else {
            decoded.push_str(value);
        }
        decoded_elements.push(element.map_range(|_| (start..decoded.len()).into()));
        offset = range.end;
    }
    decoded.push_str(&text[offset..]);
    (decoded, decoded_elements)
}

#[cfg(test)]
#[path = "project_references_tests.rs"]
mod tests;
