use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn unread_binding_supports_remapping_chords_and_unbinding() {
    for (configured, hint) in [
        (json!("f12"), Some("f12")),
        (json!(["ctrl-x u", "f12"]), Some("⌃x u")),
        (json!([]), None),
    ] {
        let config =
            serde_json::from_value(json!({"global": {"toggle_unread": configured}})).unwrap();
        let runtime = RuntimeKeymap::from_config(&config).unwrap();
        assert_eq!(
            runtime
                .primary_hint(KeymapContext::Global, "toggle_unread")
                .map(crate::key_hint::ShortcutHint::display_label),
            hint.map(str::to_owned)
        );
        assert!(
            !runtime
                .app
                .toggle_unread
                .is_pressed(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT))
        );
        assert_eq!(
            runtime.app.toggle_unread.is_pressed(KeyCode::F(12).into()),
            hint.is_some()
        );
        if !runtime.chords.bindings.iter().any(|binding| {
            binding.action.context == KeymapContext::Global
                && binding.action.action == "toggle_unread"
        }) {
            continue;
        }
        let mut matcher = KeyChordMatcher::default();
        let contexts = KeymapContextSet::new(KeymapContext::Global);
        assert!(matches!(
            matcher.advance(
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
                &runtime.chords,
                contexts,
            ),
            KeyChordMatch::Pending(_)
        ));
        let KeyChordMatch::Completed(key) =
            matcher.advance(KeyCode::Char('u').into(), &runtime.chords, contexts)
        else {
            panic!("unread chord should complete")
        };
        assert!(runtime.app.toggle_unread.is_pressed(key));
    }
}

#[test]
fn unread_default_yields_to_existing_configured_bindings_and_chord_prefixes() {
    for (context, action) in [
        ("global", "copy"),
        ("editor", "undo"),
        ("list", "move_up"),
        ("approval", "approve"),
    ] {
        for binding in ["alt-m", "alt-m x"] {
            let config = serde_json::from_value(json!({context: {action: binding}})).unwrap();
            let runtime = RuntimeKeymap::from_config(&config)
                .expect("existing custom shortcuts retain priority over the new default");
            assert!(runtime.app.toggle_unread.is_empty());
        }
    }
}

#[test]
fn explicit_unread_binding_rejects_conflicts_with_other_actions_and_the_editor() {
    for (context, action) in [
        ("global", "copy"),
        ("editor", "undo"),
        ("composer", "history_search_next"),
    ] {
        for binding in ["f12", "ctrl-x u"] {
            let mut config = json!({context: {action: binding}});
            config["global"]["toggle_unread"] = json!(binding);
            let config = serde_json::from_value(config).unwrap();
            let error = RuntimeKeymap::from_config(&config).expect_err("conflicting shortcuts");
            assert!(
                error.contains("toggle_unread") && error.contains(action),
                "{error}"
            );
        }
    }
}
