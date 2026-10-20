use super::*;
use crate::mention_codec::LinkedMention;
use pretty_assertions::assert_eq;

#[test]
fn project_bindings_survive_history_with_spaces_unicode_and_parentheses() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("other project (界)");
    let absolute = AbsolutePathBuf::from_absolute_path(&path).unwrap();
    let target = project_target(&path.to_string_lossy());
    assert_eq!(project_path(&target), Some(absolute));
    let name = project_name(&path.to_string_lossy());
    let text = format!("Compare @{name}");
    let encoded = crate::mention_codec::encode_history_mentions(
        &text,
        &[LinkedMention {
            sigil: '@',
            mention: name.clone(),
            path: target.clone(),
        }],
    );
    let decoded = crate::mention_codec::decode_history_mentions_with_at_mentions(
        &encoded, /*at_mentions_enabled*/ true,
    );
    assert_eq!(decoded.text, text);
    assert_eq!(
        decoded.mentions,
        vec![LinkedMention {
            sigil: '@',
            mention: name,
            path: target
        }]
    );
}

#[test]
fn only_structured_project_selections_attach_deduplicated_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = AbsolutePathBuf::from_absolute_path(directory.path()).unwrap();
    let mention = UserInput::Mention {
        name: "project-example".to_string(),
        path: project_target(&directory.path().to_string_lossy()),
    };
    let items = vec![
        UserInput::Text {
            text: "@project-example".to_string(),
            text_elements: Vec::new(),
        },
        mention.clone(),
        mention,
        UserInput::Mention {
            name: "plugin".to_string(),
            path: "plugin://example@test".to_string(),
        },
    ];
    assert_eq!(selected_projects(&items), vec![root]);
    assert_eq!(selected_projects(&items[..1]), Vec::new());
    assert_eq!(project_path("project://relative"), None);
}
