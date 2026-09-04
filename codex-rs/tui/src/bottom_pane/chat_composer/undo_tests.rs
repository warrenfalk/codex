use std::path::PathBuf;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc::unbounded_channel;

use super::ChatComposer;
use super::InputResult;
use super::LARGE_PASTE_CHAR_THRESHOLD;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::MentionBinding;
use crate::prompt_rewrite::RewrittenComposerDraft;

fn composer() -> ChatComposer {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    ChatComposer::new(
        /*has_input_focus*/ true,
        AppEventSender::new(tx),
        /*enhanced_keys_supported*/ true,
        "Ask Codex to do anything".to_string(),
        /*disable_paste_burst*/ true,
    )
}

fn composer_with_paste_burst() -> ChatComposer {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    ChatComposer::new(
        /*has_input_focus*/ true,
        AppEventSender::new(tx),
        /*enhanced_keys_supported*/ true,
        "Ask Codex to do anything".to_string(),
        /*disable_paste_burst*/ false,
    )
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn type_text(composer: &mut ChatComposer, text: &str) {
    for ch in text.chars() {
        let _ = composer.handle_key_event(key(KeyCode::Char(ch), KeyModifiers::NONE));
    }
}

fn undo(composer: &mut ChatComposer) {
    let _ = composer.handle_key_event(key(KeyCode::Char('u'), KeyModifiers::ALT));
}

fn redo(composer: &mut ChatComposer) {
    let _ = composer.handle_key_event(key(KeyCode::Char('e'), KeyModifiers::ALT));
}

#[test]
fn punctuation_and_newline_end_typing_units_after_the_stroke() {
    let mut composer = composer();

    type_text(&mut composer, "hello.world");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "hello.");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");

    type_text(&mut composer, "line");
    let _ = composer.handle_key_event(key(KeyCode::Enter, KeyModifiers::SHIFT));
    type_text(&mut composer, "next");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "line\n");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");
}

#[test]
fn cursor_movement_splits_typing_units_without_becoming_an_edit() {
    let mut composer = composer();

    type_text(&mut composer, "abc");
    let _ = composer.handle_key_event(key(KeyCode::Left, KeyModifiers::NONE));
    type_text(&mut composer, "X");
    assert_eq!(composer.current_text(), "abXc");

    undo(&mut composer);
    assert_eq!(composer.current_text(), "abc");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");
}

#[test]
fn paste_burst_timing_does_not_split_contiguous_typing() {
    let mut composer = composer_with_paste_burst();

    for ch in ['a', 'b', '.', 'c'] {
        let _ = composer.handle_key_event(key(KeyCode::Char(ch), KeyModifiers::NONE));
        std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
        assert!(composer.flush_paste_burst_if_due());
    }
    assert_eq!(composer.current_text(), "ab.c");

    undo(&mut composer);
    assert_eq!(composer.current_text(), "ab.");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");
    redo(&mut composer);
    assert_eq!(composer.current_text(), "ab.");

    let mut composer = composer_with_paste_burst();
    let _ = composer.handle_key_event(key(KeyCode::Char('a'), KeyModifiers::NONE));
    let _ = composer.handle_key_event(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "a");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");
}

#[test]
fn repeated_backward_and_forward_deletes_coalesce_separately() {
    let mut composer = composer();
    composer.set_text_content("abcdef".to_string(), Vec::new(), Vec::new());
    composer.move_cursor_to_end();

    let _ = composer.handle_key_event(key(KeyCode::Backspace, KeyModifiers::NONE));
    let _ = composer.handle_key_event(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "abcd");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "abcdef");

    let _ = composer.handle_key_event(key(KeyCode::Home, KeyModifiers::NONE));
    let _ = composer.handle_key_event(key(KeyCode::Delete, KeyModifiers::NONE));
    let _ = composer.handle_key_event(key(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "cdef");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "abcdef");
}

#[test]
fn divergent_edit_clears_redo() {
    let mut composer = composer();
    type_text(&mut composer, "abc");
    undo(&mut composer);
    type_text(&mut composer, "x");

    redo(&mut composer);
    assert_eq!(composer.current_text(), "x");
}

#[test]
fn ctrl_c_clear_round_trips_the_complete_draft() {
    let mut composer = composer();
    composer.set_text_content_with_mention_bindings(
        "$drive ".to_string(),
        Vec::new(),
        Vec::new(),
        vec![MentionBinding {
            sigil: '$',
            mention: "drive".to_string(),
            path: "app://drive".to_string(),
        }],
    );
    composer.handle_paste("x".repeat(LARGE_PASTE_CHAR_THRESHOLD + 1));
    composer.attach_image(PathBuf::from("/tmp/local.png"));
    composer.set_remote_image_urls(vec!["https://example.com/remote.png".to_string()]);
    composer.set_current_cursor(2);
    let before_clear = composer.snapshot_draft();

    assert!(composer.clear_for_ctrl_c().is_some());
    let after_clear = composer.snapshot_draft();
    assert_eq!(after_clear.text, "");

    undo(&mut composer);
    assert_eq!(composer.snapshot_draft(), before_clear);
    redo(&mut composer);
    assert_eq!(composer.snapshot_draft(), after_clear);
}

#[test]
fn prompt_rewrite_is_one_full_rich_draft_undo_unit() {
    let mut composer = composer();
    composer.set_remote_image_urls(vec!["https://example.com/remote.png".to_string()]);
    composer.set_text_content_with_mention_bindings(
        "$drive explain [Image #2]".to_string(),
        vec![
            codex_protocol::user_input::TextElement::new((0..6).into(), Some("$drive".to_string())),
            codex_protocol::user_input::TextElement::new(
                (15..25).into(),
                Some("[Image #2]".to_string()),
            ),
        ],
        vec![PathBuf::from("/tmp/local.png")],
        vec![MentionBinding {
            sigil: '$',
            mention: "drive".to_string(),
            path: "app://drive".to_string(),
        }],
    );
    composer.set_current_cursor(3);
    let before = composer.snapshot_draft();
    let expected = composer.prepare_prompt_rewrite_snapshot();

    assert!(composer.apply_prompt_rewrite(
        &expected,
        RewrittenComposerDraft {
            text: "Explain [Image #2] using $drive.".to_string(),
            text_elements: vec![
                codex_protocol::user_input::TextElement::new(
                    (8..18).into(),
                    Some("[Image #2]".to_string()),
                ),
                codex_protocol::user_input::TextElement::new(
                    (25..31).into(),
                    Some("$drive".to_string()),
                ),
            ],
        },
    ));

    undo(&mut composer);
    assert_eq!(composer.snapshot_draft(), before);
}

#[test]
fn prompt_rewrite_ignores_cursor_motion_but_rejects_content_edits() {
    let mut composer = composer();
    composer.set_text_content("wordy draft".to_string(), Vec::new(), Vec::new());
    let before = composer.snapshot_draft();
    let expected = composer.prepare_prompt_rewrite_snapshot();
    composer.set_current_cursor(2);

    assert!(composer.apply_prompt_rewrite(
        &expected,
        RewrittenComposerDraft {
            text: "clear draft".to_string(),
            text_elements: Vec::new(),
        },
    ));
    undo(&mut composer);
    assert_eq!(composer.snapshot_draft(), before);

    let expected = composer.prepare_prompt_rewrite_snapshot();
    type_text(&mut composer, " changed");
    let changed = composer.current_text();
    assert!(!composer.apply_prompt_rewrite(
        &expected,
        RewrittenComposerDraft {
            text: "discarded".to_string(),
            text_elements: Vec::new(),
        },
    ));
    assert_eq!(composer.current_text(), changed);
}

#[test]
fn prompt_rewrite_is_one_vim_undo_unit() {
    let mut composer = composer();
    composer.set_text_content("wordy draft".to_string(), Vec::new(), Vec::new());
    composer.set_current_cursor(2);
    composer.set_vim_enabled(/*enabled*/ true);
    let before = composer.snapshot_draft();
    let expected = composer.prepare_prompt_rewrite_snapshot();

    assert!(composer.apply_prompt_rewrite(
        &expected,
        RewrittenComposerDraft {
            text: "clear draft".to_string(),
            text_elements: Vec::new(),
        },
    ));

    let _ = composer.handle_key_event(key(KeyCode::Char('u'), KeyModifiers::NONE));
    assert_eq!(composer.snapshot_draft(), before);
}

#[test]
fn external_editor_is_atomic_and_identical_round_trip_adds_no_entry() {
    let mut composer = composer();
    type_text(&mut composer, "abc");

    composer.apply_external_edit("!git status".to_string());
    composer.apply_external_edit("!git status".to_string());
    undo(&mut composer);
    assert_eq!(composer.current_text(), "abc");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");

    redo(&mut composer);
    assert_eq!(composer.current_text(), "abc");
    redo(&mut composer);
    assert_eq!(composer.current_text(), "!git status");
}

#[test]
fn successful_submit_and_history_recall_establish_new_baselines() {
    let mut composer = composer();
    type_text(&mut composer, "sent");
    let (result, _) = composer.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(result, InputResult::Submitted { .. }));
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");

    type_text(&mut composer, "draft");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");
    let _ = composer.handle_key_event(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "sent");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "sent");
}

#[test]
fn successful_queue_discards_prior_undo_history() {
    let mut composer = composer();
    composer.set_queue_submissions(/*queue_submissions*/ true);
    type_text(&mut composer, "queued");

    let (result, _) = composer.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(result, InputResult::Queued { .. }));
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");
}

#[test]
fn history_search_cancel_preserves_stack_and_accept_resets_it() {
    let mut composer = composer();
    type_text(&mut composer, "alpha");
    let _ = composer.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
    type_text(&mut composer, "draft");

    let _ = composer.handle_key_event(key(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let _ = composer.handle_key_event(key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "alpha");
    let _ = composer.handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "draft");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "");

    type_text(&mut composer, "other");
    let _ = composer.handle_key_event(key(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let _ = composer.handle_key_event(key(KeyCode::Char('a'), KeyModifiers::NONE));
    let _ = composer.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "alpha");
    undo(&mut composer);
    assert_eq!(composer.current_text(), "alpha");
}

#[test]
fn vim_insert_session_is_one_unit_and_ctrl_r_redoes() {
    let mut composer = composer();
    composer.set_vim_enabled(/*enabled*/ true);
    let _ = composer.handle_key_event(key(KeyCode::Char('i'), KeyModifiers::NONE));
    type_text(&mut composer, "hello.world");
    let _ = composer.handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE));

    let _ = composer.handle_key_event(key(KeyCode::Char('u'), KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "");
    let _ = composer.handle_key_event(key(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert_eq!(composer.current_text(), "hello.world");
    assert!(!composer.history_search_active());
}

#[test]
fn programmatic_replacement_discards_prior_undo_history() {
    let mut composer = composer();
    type_text(&mut composer, "old");
    composer.set_text_content("baseline".to_string(), Vec::new(), Vec::new());

    undo(&mut composer);
    assert_eq!(composer.current_text(), "baseline");
}
