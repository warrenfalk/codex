use super::*;
use pretty_assertions::assert_eq;

#[test]
fn duplicate_labels_keep_their_exact_project_targets_and_replay_as_selected_mentions() {
    let directory = tempfile::tempdir().unwrap();
    let paths = [
        directory.path().join("one/shared"),
        directory.path().join("two/shared"),
    ];
    let bindings: Vec<_> = paths
        .iter()
        .map(|path| MentionBinding {
            sigil: '@',
            mention: "project-shared".to_string(),
            path: crate::project_mentions::project_target(&path.to_string_lossy()),
        })
        .collect();
    let text = "@project-shared and @project-shared";
    let elements: Vec<_> = [0..15, 20..35]
        .into_iter()
        .map(|range| TextElement::new(range.into(), Some("@project-shared".to_string())))
        .collect();
    let mut items = vec![UserInput::Text {
        text: text.to_string(),
        text_elements: elements.iter().cloned().map(Into::into).collect(),
    }];
    encode(&mut items, &bindings);
    let UserInput::Text {
        text: encoded,
        text_elements,
    } = &items[0]
    else {
        panic!("expected text");
    };
    assert_eq!(
        encoded,
        &format!(
            "[@project-shared]({}) and [@project-shared]({})",
            bindings[0].path, bindings[1].path
        )
    );
    let replay_elements = text_elements.iter().cloned().map(Into::into).collect();
    assert_eq!(
        decode(encoded, replay_elements),
        (text.to_string(), elements)
    );
}
