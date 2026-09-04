use super::*;
use crate::bottom_pane::LocalImageAttachment;
use crate::bottom_pane::MentionBinding;
use codex_protocol::user_input::TextElement;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn plain_draft(text: &str) -> ComposerDraftSnapshot {
    ComposerDraftSnapshot {
        text: text.to_string(),
        text_elements: Vec::new(),
        local_images: Vec::new(),
        remote_image_urls: Vec::new(),
        mention_bindings: Vec::new(),
        pending_pastes: Vec::new(),
        startup_local_history: Vec::new(),
        last_composer_activity_at: None,
        cursor: text.len(),
    }
}

#[test]
fn restores_literal_regions_byte_for_byte() {
    let draft =
        plain_draft("please diagnose this\n```rust\nlet  x =  1;\n```\nand make the answer short");
    let request = PromptRewriteRequest::new(ThreadId::new(), draft, false).unwrap();
    let marker = &request.protected_prompt.regions[0].marker;
    let output = serde_json::json!({
        "rewritten_prompt": format!("Diagnose this:\n{marker}Keep the answer concise.")
    });

    let rewritten = request.restore_model_output(&output.to_string()).unwrap();

    assert_eq!(
        rewritten.text,
        "Diagnose this:\n```rust\nlet  x =  1;\n```\nKeep the answer concise."
    );
}

#[test]
fn protects_diffs_logs_commands_and_quoted_payloads() {
    let cases = [
        "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n",
        "2026-07-13T12:34:56Z ERROR exact  spacing\n    continuation\n",
        "$ cargo test --package exact\n",
        "> quoted  payload\n",
        "'''\nverbatim  payload\n'''\n",
    ];

    for literal in cases {
        let draft = plain_draft(&format!("Please inspect this:\n{literal}Then summarize."));
        let request = PromptRewriteRequest::new(ThreadId::new(), draft, false).unwrap();
        let markers = request
            .protected_prompt
            .regions
            .iter()
            .map(|region| region.marker.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let output = serde_json::json!({
            "rewritten_prompt": format!("Inspect this:\n{markers}Summarize it.")
        });

        let rewritten = request.restore_model_output(&output.to_string()).unwrap();
        assert!(
            rewritten.text.contains(literal),
            "literal changed: {literal:?}"
        );
    }
}

#[test]
fn restores_rich_elements_at_rewritten_offsets() {
    let mention = "$docs";
    let image = "[Image #1]";
    let text = format!("can you use {mention} to explain {image} please");
    let mention_start = text.find(mention).unwrap();
    let image_start = text.find(image).unwrap();
    let draft = ComposerDraftSnapshot {
        text,
        text_elements: vec![
            TextElement::new(
                (mention_start..mention_start + mention.len()).into(),
                Some(mention.to_string()),
            ),
            TextElement::new(
                (image_start..image_start + image.len()).into(),
                Some(image.to_string()),
            ),
        ],
        local_images: vec![LocalImageAttachment {
            placeholder: image.to_string(),
            path: PathBuf::from("image.png"),
        }],
        remote_image_urls: Vec::new(),
        mention_bindings: vec![MentionBinding {
            sigil: '$',
            mention: "docs".to_string(),
            path: "/skills/docs/SKILL.md".to_string(),
        }],
        pending_pastes: Vec::new(),
        startup_local_history: Vec::new(),
        last_composer_activity_at: None,
        cursor: 0,
    };
    let request = PromptRewriteRequest::new(ThreadId::new(), draft, false).unwrap();
    let markers = request
        .protected_prompt
        .regions
        .iter()
        .map(|region| region.marker.as_str())
        .collect::<Vec<_>>();
    let output = serde_json::json!({
        "rewritten_prompt": format!("Use {} to explain {} concisely.", markers[0], markers[1])
    });

    let rewritten = request.restore_model_output(&output.to_string()).unwrap();

    assert_eq!(rewritten.text, "Use $docs to explain [Image #1] concisely.");
    assert_eq!(
        rewritten
            .text_elements
            .iter()
            .map(|element| element.byte_range)
            .collect::<Vec<_>>(),
        vec![(4..9).into(), (21..31).into()]
    );
}

#[test]
fn rejects_missing_duplicated_corrupted_or_reordered_markers() {
    let text = "$one and $two";
    let draft = ComposerDraftSnapshot {
        text: text.to_string(),
        text_elements: vec![
            TextElement::new((0..4).into(), Some("$one".to_string())),
            TextElement::new((9..13).into(), Some("$two".to_string())),
        ],
        local_images: Vec::new(),
        remote_image_urls: Vec::new(),
        mention_bindings: vec![
            MentionBinding {
                sigil: '$',
                mention: "one".to_string(),
                path: "/one".to_string(),
            },
            MentionBinding {
                sigil: '$',
                mention: "two".to_string(),
                path: "/two".to_string(),
            },
        ],
        pending_pastes: Vec::new(),
        startup_local_history: Vec::new(),
        last_composer_activity_at: None,
        cursor: 0,
    };
    let request = PromptRewriteRequest::new(ThreadId::new(), draft, false).unwrap();
    let first = &request.protected_prompt.regions[0].marker;
    let second = &request.protected_prompt.regions[1].marker;
    let invalid = [
        "no markers".to_string(),
        format!("{first} {first} {second}"),
        format!("{} {second}", &first[..first.len() - 1]),
        format!("{second} {first}"),
    ];

    for rewritten_prompt in invalid {
        let output = serde_json::json!({ "rewritten_prompt": rewritten_prompt });
        assert!(request.restore_model_output(&output.to_string()).is_err());
    }
}

#[test]
fn excludes_empty_shell_and_recognized_slash_drafts() {
    let thread_id = ThreadId::new();
    assert_eq!(
        PromptRewriteRequest::new(thread_id, plain_draft("  "), false).unwrap_err(),
        PromptRewriteUnavailable::Empty
    );
    assert_eq!(
        PromptRewriteRequest::new(thread_id, plain_draft(" !git status"), false).unwrap_err(),
        PromptRewriteUnavailable::ShellCommand
    );
    assert_eq!(
        PromptRewriteRequest::new(thread_id, plain_draft("/review"), true).unwrap_err(),
        PromptRewriteUnavailable::SlashCommand
    );
}

#[test]
fn rejects_structured_metadata_without_a_matching_text_element() {
    let draft = ComposerDraftSnapshot {
        mention_bindings: vec![MentionBinding {
            sigil: '$',
            mention: "docs".to_string(),
            path: "/skills/docs/SKILL.md".to_string(),
        }],
        ..plain_draft("use $docs")
    };

    assert_eq!(
        PromptRewriteRequest::new(ThreadId::new(), draft, false).unwrap_err(),
        PromptRewriteUnavailable::InvalidStructuredDraft
    );
}
