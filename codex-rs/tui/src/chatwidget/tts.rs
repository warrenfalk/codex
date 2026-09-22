//! Speech mode selection and narration of live assistant messages and questions.

use super::*;
use codex_config::types::TtsMode;

impl ChatWidget {
    pub(super) fn handle_speak_command(&mut self, args: &str) {
        match args {
            "" => {
                let choices = [
                    (TtsMode::Off, "Off", "Keep messages silent."),
                    (
                        TtsMode::Final,
                        "Final",
                        "Speak final answers and interactive questions.",
                    ),
                    (
                        TtsMode::ProgressAndFinal,
                        "Progress and final",
                        "Also speak progress updates.",
                    ),
                ];
                let selected = choices
                    .iter()
                    .position(|(mode, _, _)| *mode == self.speech.mode());
                let items = choices
                    .into_iter()
                    .map(|(mode, name, description)| SelectionItem {
                        name: name.to_string(),
                        description: Some(description.to_string()),
                        is_current: mode == self.speech.mode(),
                        actions: vec![Box::new(move |tx| tx.send(AppEvent::SetTtsMode(mode)))],
                        dismiss_on_select: true,
                        ..Default::default()
                    })
                    .collect();
                self.bottom_pane.show_selection_view(SelectionViewParams {
                    title: Some("Text to speech".to_string()),
                    subtitle: Some(
                        "Choose what Codex reads aloud. Esc or Ctrl+C stops playback.".to_string(),
                    ),
                    footer_hint: Some(standard_popup_hint_line()),
                    items,
                    initial_selected_idx: selected,
                    ..Default::default()
                });
            }
            "off" => self.set_tts_mode(TtsMode::Off),
            "final" => self.set_tts_mode(TtsMode::Final),
            "progress-and-final" => self.set_tts_mode(TtsMode::ProgressAndFinal),
            "stop" => {
                self.speech.stop();
                self.add_info_message("Speech stopped.".to_string(), /*hint*/ None);
            }
            _ => self
                .add_error_message("Usage: /speak [off|final|progress-and-final|stop]".to_string()),
        }
    }

    pub(crate) fn set_tts_mode(&mut self, mode: TtsMode) {
        let read_latest = self.speech.mode() == TtsMode::Off
            && mode != TtsMode::Off
            && !self.is_user_turn_pending_or_running();
        self.speech.set_mode(mode);
        self.bottom_pane.set_tts_mode(mode);
        let label = match mode {
            TtsMode::Off => "off",
            TtsMode::Final => "final",
            TtsMode::ProgressAndFinal => "progress and final",
        };
        self.add_info_message(format!("Text to speech: {label}."), /*hint*/ None);
        if read_latest && let Some(markdown) = self.transcript.last_agent_markdown.clone() {
            // Explicit activation is a fresh request, even if this response was spoken before.
            self.speak_text("manual", &uuid::Uuid::new_v4().to_string(), &markdown);
        }
    }

    pub(crate) fn handle_speech_key(&mut self, key: KeyEvent) -> bool {
        let stop = (key.code == KeyCode::Esc && key.modifiers.is_empty())
            || matches!(key.code, KeyCode::Char('c' | 'C'))
                && key.modifiers.contains(KeyModifiers::CONTROL);
        if !stop
            || !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            || !self.speech.is_speaking()
        {
            return false;
        }
        self.speech.stop();
        self.quit_shortcut_expires_at = None;
        self.quit_shortcut_key = None;
        self.bottom_pane.clear_quit_shortcut_hint();
        self.request_redraw();
        true
    }

    pub(crate) fn inherit_tts(&mut self, previous: &mut ChatWidget) {
        previous.speech.stop();
        self.speech = std::mem::take(&mut previous.speech);
        self.bottom_pane.set_tts_mode(self.speech.mode());
    }

    pub(crate) fn on_tts_failure(&mut self, generation: uuid::Uuid, message: String) {
        if self.speech.accept_failure(generation) {
            self.bottom_pane.set_tts_mode(TtsMode::Off);
            self.add_error_message(format!(
                "Text to speech disabled: {message}. Check tui.tts.command, then use /speak to retry."
            ));
        }
    }

    pub(super) fn speak_text(&mut self, turn_id: &str, item_id: &str, markdown: &str) {
        if self.speech.mode() == TtsMode::Off {
            return;
        }
        if let Err(error) = self.speech.enqueue(
            turn_id,
            item_id,
            crate::tts::spoken_text(markdown),
            &self.local_settings.tui.tts,
            &self.app_event_tx,
        ) {
            self.speech.set_mode(TtsMode::Off);
            self.bottom_pane.set_tts_mode(TtsMode::Off);
            self.add_error_message(format!(
                "Text to speech disabled: {error}. Use /speak to retry."
            ));
        }
    }

    pub(super) fn speak_input_questions(&mut self, params: &ToolRequestUserInputParams) {
        let mut parts = Vec::new();
        for question in &params.questions {
            parts.push(question.question.clone());
            if let Some(options) = &question.options {
                for (index, option) in options.iter().enumerate() {
                    let number = index + 1;
                    parts.push(format!(
                        "Option {number}: {}. {}",
                        option.label, option.description
                    ));
                }
            }
        }
        self.speak_text(&params.turn_id, &params.item_id, &parts.join("\n\n"));
    }
}

#[cfg(test)]
#[path = "tts_tests.rs"]
mod tests;
