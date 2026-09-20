use super::StatusLineItem;
use super::status_line_from_segments;
use crate::bottom_pane::StatusSurfacePreviewData;
use pretty_assertions::assert_eq;

#[test]
fn model_items_strip_only_one_leading_gpt_prefix() {
    for item in [
        StatusLineItem::ModelName,
        StatusLineItem::ModelWithReasoning,
    ] {
        for (model, expected) in [
            ("gpt-5.4", "5.4"),
            ("gpt-5.4 xhigh fast", "5.4 xhigh fast"),
            ("gpt-gpt-custom", "gpt-custom"),
            ("custom-gpt-model", "custom-gpt-model"),
            ("o3 high", "o3 high"),
        ] {
            let line = status_line_from_segments(
                [(item, model.to_string())],
                /*use_theme_colors*/ false,
            )
            .expect("model status line");

            assert_eq!(line.to_string(), expected, "{item}: {model}");
        }
    }
}

#[test]
fn current_directory_keeps_the_last_two_components() {
    let separator = std::path::MAIN_SEPARATOR_STR;
    for (path, expected) in [
        ("~/source/codex-cli/codex", "codex-cli/codex"),
        ("~/mygame", "~/mygame"),
        ("/warren-desktop", "/warren-desktop"),
        ("/tmp/project", "tmp/project"),
        ("/source/codex-cli/codex/", "codex-cli/codex"),
        ("~/source/日本語/my game", "日本語/my game"),
        ("~", "~"),
        ("/", "/"),
    ] {
        let path = path.replace('/', separator);
        let line = status_line_from_segments(
            [(StatusLineItem::CurrentDir, path.clone())],
            /*use_theme_colors*/ false,
        )
        .expect("directory status line");

        assert_eq!(line.to_string(), expected.replace('/', separator), "{path}");
    }
}

#[cfg(windows)]
#[test]
fn current_directory_preserves_shallow_windows_roots() {
    for (path, expected) in [
        (r"C:\source\codex-cli\codex", r"codex-cli\codex"),
        (r"C:\mygame", r"C:\mygame"),
        (r"C:\", r"C:\"),
        (r"\\server\share\project", r"\\server\share\project"),
        (r"\\server\share\parent\project", r"parent\project"),
    ] {
        let line = status_line_from_segments(
            [(StatusLineItem::CurrentDir, path.to_string())],
            /*use_theme_colors*/ false,
        )
        .expect("directory status line");

        assert_eq!(line.to_string(), expected, "{path}");
    }
}

#[test]
fn status_line_and_setup_preview_use_the_same_compact_values() {
    let values = [
        (StatusLineItem::ModelWithReasoning, "gpt-5.4 xhigh fast"),
        (StatusLineItem::ContextRemaining, "Context 100% left"),
        (StatusLineItem::CurrentDir, "~/source/codex-cli/codex"),
        (StatusLineItem::GitBranch, "gpt-feature"),
    ];
    let preview = StatusSurfacePreviewData::from_iter(
        values
            .iter()
            .map(|(item, value)| (item.preview_item(), *value)),
    );
    for use_theme_colors in [false, true] {
        let rendered = status_line_from_segments(
            values
                .iter()
                .map(|(item, value)| (*item, value.to_string())),
            use_theme_colors,
        )
        .expect("compact status line");
        let preview = preview
            .status_line_for_items(values.iter().map(|(item, _)| *item), use_theme_colors)
            .expect("compact preview");

        assert_eq!(rendered, preview);
        assert_eq!(
            rendered.to_string().replace('\\', "/"),
            "5.4 xhigh fast · 100% left · codex-cli/codex · gpt-feature"
        );
    }
}
