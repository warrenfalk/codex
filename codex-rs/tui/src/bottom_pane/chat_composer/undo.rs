//! Bounded full-draft undo and redo history for one chat composer instance.

use std::collections::VecDeque;
use std::time::Instant;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use super::super::paste_burst::FlushResult;
use super::ChatComposer;
use super::ComposerDraft;
use crate::key_hint::KeyBindingListExt;

const MAX_STACK_ENTRIES: usize = 100;
const MAX_STACK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditGroup {
    Typing,
    DeleteBackward,
    DeleteForward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditKind {
    Grouped { group: EditGroup, close_after: bool },
    Atomic,
    Boundary,
}

impl EditKind {
    fn typing(ch: char) -> Self {
        Self::Grouped {
            group: EditGroup::Typing,
            close_after: matches!(ch, '.' | ',' | ';' | ':' | '?' | '!'),
        }
    }
}

#[derive(Debug)]
struct ActiveGroup {
    group: EditGroup,
    initial: ComposerDraft,
    recorded: bool,
}

#[derive(Debug, Default)]
struct DraftStack {
    drafts: VecDeque<ComposerDraft>,
    bytes: usize,
}

impl DraftStack {
    fn clear(&mut self) {
        self.drafts.clear();
        self.bytes = 0;
    }

    fn push(&mut self, draft: ComposerDraft) {
        self.bytes = self.bytes.saturating_add(draft.approximate_size());
        self.drafts.push_back(draft);
        while self.drafts.len() > 1
            && (self.drafts.len() > MAX_STACK_ENTRIES || self.bytes > MAX_STACK_BYTES)
        {
            if let Some(removed) = self.drafts.pop_front() {
                self.bytes = self.bytes.saturating_sub(removed.approximate_size());
            }
        }
    }

    fn pop(&mut self) -> Option<ComposerDraft> {
        let draft = self.drafts.pop_back()?;
        self.bytes = self.bytes.saturating_sub(draft.approximate_size());
        Some(draft)
    }
}

#[derive(Debug, Default)]
pub(super) struct UndoState {
    undo: DraftStack,
    redo: DraftStack,
    active_group: Option<ActiveGroup>,
    revision: u64,
}

impl UndoState {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn reset(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.active_group = None;
        self.bump_revision();
    }

    pub(super) fn boundary(&mut self) {
        self.active_group = None;
    }

    pub(super) fn active_group_is(&self, group: EditGroup) -> bool {
        self.active_group
            .as_ref()
            .is_some_and(|active| active.group == group)
    }

    pub(super) fn record(&mut self, before: ComposerDraft, after: &ComposerDraft, kind: EditKind) {
        match kind {
            EditKind::Boundary => self.boundary(),
            EditKind::Atomic => {
                self.boundary();
                if !before.same_content(after) {
                    self.push_new_edit(before);
                }
            }
            EditKind::Grouped { group, close_after } => {
                let event_changed = !before.same_content(after);
                if !self.active_group_is(group) {
                    self.active_group = Some(ActiveGroup {
                        group,
                        initial: before,
                        recorded: false,
                    });
                }

                let changed = self
                    .active_group
                    .as_ref()
                    .is_some_and(|active| !active.initial.same_content(after));
                let should_record = self
                    .active_group
                    .as_ref()
                    .is_some_and(|active| changed && !active.recorded);
                if should_record
                    && let Some(initial) = self
                        .active_group
                        .as_ref()
                        .map(|active| active.initial.clone())
                {
                    self.push_new_edit(initial);
                    if let Some(active) = self.active_group.as_mut() {
                        active.recorded = true;
                    }
                }

                if close_after && event_changed {
                    self.boundary();
                }
            }
        }
    }

    pub(super) fn undo(&mut self, current: ComposerDraft) -> Option<ComposerDraft> {
        self.boundary();
        let target = self.undo.pop()?;
        self.redo.push(current);
        self.bump_revision();
        Some(target)
    }

    pub(super) fn redo(&mut self, current: ComposerDraft) -> Option<ComposerDraft> {
        self.boundary();
        let target = self.redo.pop()?;
        self.undo.push(current);
        self.bump_revision();
        Some(target)
    }

    fn push_new_edit(&mut self, before: ComposerDraft) {
        self.undo.push(before);
        self.redo.clear();
        self.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

impl ComposerDraft {
    fn same_content(&self, other: &Self) -> bool {
        self.text == other.text
            && self.text_elements == other.text_elements
            && self.local_image_paths == other.local_image_paths
            && self.remote_image_urls == other.remote_image_urls
            && self.mention_bindings == other.mention_bindings
            && self.pending_pastes == other.pending_pastes
    }

    fn approximate_size(&self) -> usize {
        let local_image_bytes = self
            .local_image_paths
            .iter()
            .map(|path| path.to_string_lossy().len())
            .sum::<usize>();
        let remote_image_bytes = self
            .remote_image_urls
            .iter()
            .map(String::len)
            .sum::<usize>();
        let mention_bytes = self
            .mention_bindings
            .iter()
            .map(|binding| binding.mention.len() + binding.path.len())
            .sum::<usize>();
        let pending_paste_bytes = self
            .pending_pastes
            .iter()
            .map(|(placeholder, pasted)| placeholder.len() + pasted.len())
            .sum::<usize>();

        self.text.len()
            + self.text_elements.len()
                * std::mem::size_of::<codex_protocol::user_input::TextElement>()
            + local_image_bytes
            + remote_image_bytes
            + mention_bytes
            + pending_paste_bytes
            + std::mem::size_of::<usize>()
    }
}

impl ChatComposer {
    pub(super) fn editor_undo_checkpoint(&self) -> Option<(ComposerDraft, u64)> {
        (!self.draft.textarea.is_vim_enabled())
            .then(|| (self.snapshot_draft(), self.undo.revision()))
    }

    pub(super) fn record_edit_from(
        &mut self,
        checkpoint: Option<(ComposerDraft, u64)>,
        kind: EditKind,
    ) {
        let Some((before, revision)) = checkpoint else {
            return;
        };
        if self.undo.revision() != revision {
            return;
        }
        let after = self.snapshot_draft();
        self.undo.record(before, &after, kind);
    }

    pub(super) fn apply_undo(&mut self) -> bool {
        self.flush_paste_burst_before_modified_input_for_undo();
        let current = self.snapshot_draft();
        let Some(target) = self.undo.undo(current) else {
            return false;
        };
        self.restore_draft(target);
        self.history.reset_navigation();
        true
    }

    pub(super) fn apply_redo(&mut self) -> bool {
        self.flush_paste_burst_before_modified_input_for_undo();
        let current = self.snapshot_draft();
        let Some(target) = self.undo.redo(current) else {
            return false;
        };
        self.restore_draft(target);
        self.history.reset_navigation();
        true
    }

    pub(super) fn flush_due_paste_burst_for_undo(&mut self, now: Instant) -> bool {
        let flush_result = self.draft.paste_burst.flush_if_due(now);
        let undo_checkpoint = self.editor_undo_checkpoint();
        let kind = match flush_result {
            FlushResult::Paste(pasted) => {
                self.handle_paste_raw(pasted);
                EditKind::Atomic
            }
            FlushResult::Typed(ch) => {
                self.insert_str_raw(ch.to_string().as_str());
                EditKind::typing(ch)
            }
            FlushResult::None => return false,
        };
        self.record_edit_from(undo_checkpoint, kind);
        true
    }

    pub(super) fn flush_paste_burst_before_key(&mut self, key_event: KeyEvent) -> bool {
        let plain_char = matches!(
            key_event,
            KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            } if !ch.is_control()
                && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        );
        let pending_before_non_ascii = matches!(key_event.code, KeyCode::Char(ch) if !ch.is_ascii())
            && self.draft.paste_burst.has_pending_first_char();
        if key_event.code != KeyCode::Enter && !plain_char {
            let flushed = self.flush_paste_burst_before_modified_input_for_undo();
            self.draft.paste_burst.clear_window_after_non_char();
            return flushed;
        }
        pending_before_non_ascii && self.flush_paste_burst_before_modified_input_for_undo()
    }

    pub(super) fn flush_paste_burst_before_modified_input_for_undo(&mut self) -> bool {
        let buffered_paste = self.draft.paste_burst.has_buffered_paste();
        let Some(pasted) = self.draft.paste_burst.flush_before_modified_input() else {
            return false;
        };
        let typed = if buffered_paste {
            None
        } else {
            pasted.chars().next()
        };
        let undo_checkpoint = self.editor_undo_checkpoint();
        self.handle_paste_raw(pasted);
        let kind = typed.map_or(EditKind::Atomic, EditKind::typing);
        self.record_edit_from(undo_checkpoint, kind);
        true
    }

    fn is_editor_movement_key(&self, key_event: KeyEvent) -> bool {
        self.editor_keymap.move_left.is_pressed(key_event)
            || self.editor_keymap.move_right.is_pressed(key_event)
            || self.editor_keymap.move_up.is_pressed(key_event)
            || self.editor_keymap.move_down.is_pressed(key_event)
            || self.editor_keymap.move_word_left.is_pressed(key_event)
            || self.editor_keymap.move_word_right.is_pressed(key_event)
            || self.editor_keymap.move_line_start.is_pressed(key_event)
            || self.editor_keymap.move_line_end.is_pressed(key_event)
    }

    pub(super) fn edit_kind_for_key(&self, key_event: KeyEvent) -> EditKind {
        debug_assert!(!self.draft.textarea.is_vim_enabled());
        if self.attachments.selected_remote_image_index.is_some()
            && matches!(key_event.code, KeyCode::Backspace | KeyCode::Delete)
        {
            return EditKind::Atomic;
        }

        if self.is_editor_movement_key(key_event) {
            return EditKind::Boundary;
        }
        if self.editor_keymap.delete_backward.is_pressed(key_event) {
            return EditKind::Grouped {
                group: EditGroup::DeleteBackward,
                close_after: false,
            };
        }
        if self.editor_keymap.delete_forward.is_pressed(key_event) {
            return EditKind::Grouped {
                group: EditGroup::DeleteForward,
                close_after: false,
            };
        }
        if self.editor_keymap.insert_newline.is_pressed(key_event) {
            return EditKind::Grouped {
                group: EditGroup::Typing,
                close_after: true,
            };
        }
        if let KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
            ..
        } = key_event
        {
            return EditKind::typing(ch);
        }

        EditKind::Atomic
    }
}
