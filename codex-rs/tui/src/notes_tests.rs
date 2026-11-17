use super::*;
use pretty_assertions::assert_eq;

fn note(id: &str) -> NoteToSelf {
    NoteToSelf {
        id: id.to_string(),
        text: format!("Note {id}"),
    }
}

#[test]
fn notes_merge_complete_history_with_live_arrivals_without_duplicates() {
    let mut notes = NotesState::default();
    let generation = notes.begin_load();
    notes.record(note("recent"));
    notes.record(note("new"));
    notes.record(note("new"));

    assert!(notes.finish_load(
        generation,
        Ok(vec![note("recent"), note("old"), note("old")]),
    ));

    assert_eq!(
        notes.entries,
        vec![note("new"), note("recent"), note("old")]
    );
    assert_eq!(
        notes.indicator(),
        Some(Line::from("3 notes to self · /nts to view".magenta()))
    );
}

#[test]
fn notes_ignore_obsolete_history_after_reload() {
    let mut notes = NotesState::default();
    let old = notes.begin_load();
    let current = notes.begin_load();

    assert!(!notes.finish_load(old, Ok(vec![note("removed")])));
    assert!(notes.loading);
    assert!(notes.finish_load(current, Ok(Vec::new())));
    assert_eq!(notes.entries, Vec::new());
    assert_eq!(notes.indicator(), None);
}

#[test]
fn notes_retry_preserves_live_notes_and_replaces_failure_with_complete_count() {
    let mut notes = NotesState::default();
    let generation = notes.begin_load();
    notes.record(note("live"));
    assert_eq!(
        notes.indicator(),
        Some(Line::from("Notes to self · /nts to view".magenta()))
    );
    notes.finish_load(generation, Err("connection lost".to_string()));
    assert_eq!(
        notes.indicator(),
        Some(Line::from("Notes unavailable · /nts to retry".magenta()))
    );

    let retry = notes.begin_load();
    notes.finish_load(retry, Ok(vec![note("live")]));
    assert_eq!(notes.entries, vec![note("live")]);
    assert_eq!(
        notes.indicator(),
        Some(Line::from("1 note to self · /nts to view".magenta()))
    );
}

#[tokio::test]
async fn notes_cancel_loading_when_the_session_is_replaced() {
    let task = tokio::spawn(std::future::pending::<()>());
    let mut notes = NotesState::default();
    notes.task = Some(task.abort_handle());
    drop(notes);
    assert!(task.await.expect_err("load cancelled").is_cancelled());
}
