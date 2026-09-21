//! Ordered, cancellable speech through a local command. No speech text is interpreted by a shell.

use std::collections::VecDeque;
use std::process::Stdio;

use anyhow::Context;
use codex_config::types::TtsConfig;
use codex_config::types::TtsMode;
use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

const MAX_QUEUED_MESSAGES: usize = 32;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_RECENT_ITEMS: usize = 256;

#[derive(Default)]
pub(crate) struct Speech {
    mode: TtsMode,
    generation: Uuid,
    recent_items: VecDeque<(String, String)>,
    sender: Option<mpsc::Sender<String>>,
    worker: Option<JoinHandle<()>>,
}

impl Speech {
    pub(crate) fn mode(&self) -> TtsMode {
        self.mode
    }

    pub(crate) fn set_mode(&mut self, mode: TtsMode) {
        if self.mode != mode {
            self.stop();
            self.mode = mode;
        }
    }

    /// Cancel this client's active command and discard its queue, retaining the mode.
    pub(crate) fn stop(&mut self) {
        self.generation = Uuid::new_v4();
        self.sender = None;
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }

    pub(crate) fn accept_failure(&mut self, generation: Uuid) -> bool {
        if self.generation != generation {
            return false;
        }
        self.set_mode(TtsMode::Off);
        true
    }

    pub(crate) fn enqueue(
        &mut self,
        turn_id: &str,
        item_id: &str,
        text: String,
        config: &TtsConfig,
        events: &AppEventSender,
    ) -> anyhow::Result<()> {
        if self.mode == TtsMode::Off || text.trim().is_empty() {
            return Ok(());
        }
        let key = (turn_id.to_string(), item_id.to_string());
        if self.recent_items.contains(&key) {
            return Ok(());
        }
        self.recent_items.push_back(key);
        if self.recent_items.len() > MAX_RECENT_ITEMS {
            self.recent_items.pop_front();
        }
        anyhow::ensure!(
            text.len() <= MAX_MESSAGE_BYTES,
            "message exceeds the 64 KiB speech limit"
        );
        let (program, args) = config
            .command
            .split_first()
            .context("tui.tts.command is empty")?;
        anyhow::ensure!(
            !program.trim().is_empty(),
            "tui.tts.command has an empty executable"
        );
        if self.sender.is_none() {
            let (sender, mut receiver) = mpsc::channel::<String>(MAX_QUEUED_MESSAGES);
            let program = program.clone();
            let args = args.to_vec();
            let events = events.clone();
            let generation = Uuid::new_v4();
            self.generation = generation;
            self.sender = Some(sender);
            self.worker = Some(tokio::spawn(async move {
                while let Some(text) = receiver.recv().await {
                    let result = async {
                        let mut child = Command::new(&program)
                            .args(&args)
                            .stdin(Stdio::piped())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .kill_on_drop(true)
                            .spawn()
                            .with_context(|| format!("could not start {program:?}"))?;
                        let mut stdin =
                            child.stdin.take().context("speech command has no stdin")?;
                        stdin
                            .write_all(text.as_bytes())
                            .await
                            .context("could not write speech text")?;
                        drop(stdin);
                        let status = child
                            .wait()
                            .await
                            .context("could not wait for speech command")?;
                        anyhow::ensure!(status.success(), "{program:?} exited with {status}");
                        Ok::<(), anyhow::Error>(())
                    }
                    .await;
                    if let Err(error) = result {
                        events.send(AppEvent::TtsFailed {
                            generation,
                            message: format!("{error:#}"),
                        });
                        break;
                    }
                }
            }));
        }
        self.sender
            .as_ref()
            .context("speech worker is unavailable")?
            .try_send(text)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => anyhow::anyhow!("speech queue is full"),
                mpsc::error::TrySendError::Closed(_) => anyhow::anyhow!("speech command stopped"),
            })
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Narrate prose and link labels, leaving code blocks, HTML, and formatting out of speech.
pub(crate) fn spoken_text(markdown: &str) -> String {
    let mut text = String::new();
    let mut in_code_block = false;
    for event in Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    ) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                text.push('\n');
            }
            Event::Text(content) | Event::Code(content) if !in_code_block => {
                text.push_str(&content)
            }
            Event::SoftBreak if !in_code_block => text.push(' '),
            Event::HardBreak | Event::Rule if !in_code_block => text.push('\n'),
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableRow,
            ) if !in_code_block => text.push('\n'),
            Event::End(TagEnd::TableCell) if !in_code_block => text.push_str(". "),
            Event::Start(_)
            | Event::End(_)
            | Event::Text(_)
            | Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::TaskListMarker(_) => {}
        }
    }
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "tts_tests.rs"]
mod tests;
