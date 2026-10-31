use super::*;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn selecting_and_removing_project_updates_only_draft_bindings() {
    let (tx, mut events) = unbounded_channel();
    let mut composer = ChatComposer::new(
        /*has_input_focus*/ true,
        AppEventSender::new(tx),
        /*enhanced_keys_supported*/ false,
        "Ask Codex".to_string(),
        /*disable_paste_burst*/ true,
    );
    composer.set_mentions_v2_enabled(/*enabled*/ true);
    composer.set_text_content("@".to_string(), Vec::new(), Vec::new());
    let directory = tempfile::tempdir().unwrap();
    let target = crate::project_mentions::project_target(&directory.path().to_string_lossy());
    composer.insert_selected_mention(0..1, "@project-other", Some(&target));
    assert_eq!(
        composer.snapshot_mention_bindings(),
        vec![MentionBinding {
            sigil: '@',
            mention: "project-other".to_string(),
            path: target,
        }]
    );
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).all(|event| matches!(
            event,
            AppEvent::StartFileSearch(_) | AppEvent::StartProjectSearch(_)
        ))
    );
    composer.set_text_content("Compare the projects".to_string(), Vec::new(), Vec::new());
    assert_eq!(composer.snapshot_mention_bindings(), Vec::new());
}
