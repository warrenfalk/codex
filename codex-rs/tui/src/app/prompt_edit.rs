//! Choosing and applying edits to earlier prompts without changing their attachments.

use super::session_lifecycle::ThreadAttachPresentation;
use super::*;
use crate::app_backtrack::BacktrackSelection;
use crate::app_server_session::ForkGoalContinuation;
use crate::bottom_pane::SelectionDescriptionLayout;
use crate::bottom_pane::popup_consts::picker_hint_line_for_keymap;
use crate::chatwidget::UserMessage;
use codex_app_server_client::AppServerEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptEditMode {
    Replace,
    Fork,
}

impl App {
    pub(crate) fn show_prompt_edit_menu(&mut self, selection: BacktrackSelection) {
        let Some(index) = crate::app_backtrack::nth_user_position(
            &self.transcript_cells,
            selection.nth_user_message,
        ) else {
            return;
        };
        let selected_cell = Arc::clone(&self.transcript_cells[index]);
        let items = [
            (
                PromptEditMode::Replace,
                "Edit this conversation",
                "Replace this prompt and discard everything after it.",
            ),
            (
                PromptEditMode::Fork,
                "Edit in a new conversation",
                "Keep the original conversation and continue on a new branch.",
            ),
        ]
        .into_iter()
        .map(|(mode, name, description)| {
            let selection = selection.clone();
            let selected_cell = Arc::clone(&selected_cell);
            SelectionItem {
                name: name.to_string(),
                description: Some(description.to_string()),
                actions: vec![Box::new(move |tx| {
                    let BacktrackSelection {
                        thread_id,
                        nth_user_message: _,
                        prompt,
                    } = selection.clone();
                    tx.send(match mode {
                        PromptEditMode::Replace => AppEvent::RevertSessionForPromptEdit {
                            thread_id,
                            selected_cell: Arc::clone(&selected_cell),
                            prompt,
                        },
                        PromptEditMode::Fork => AppEvent::ForkSessionForPromptEdit {
                            thread_id,
                            selected_cell: Arc::clone(&selected_cell),
                            prompt,
                        },
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();
        self.chat_widget.show_selection_view(SelectionViewParams {
            title: Some("Edit previous prompt".to_string()),
            footer_hint: Some(picker_hint_line_for_keymap(&self.keymap.list)),
            items,
            description_layout: SelectionDescriptionLayout::HideWhenNarrow {
                min_description_width: 28,
            },
            ..SelectionViewParams::picker()
        });
    }

    pub(super) async fn edit_previous_prompt(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        selected_cell: Arc<dyn HistoryCell>,
        mut prompt: UserMessage,
        mode: PromptEditMode,
    ) -> Result<AppRunControl> {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return Ok(AppRunControl::Continue);
        }
        if self.app_server_target.uses_remote_workspace()
            && (!prompt.local_images.is_empty() || prompt.text.trim_start().starts_with(['/', '!']))
        {
            self.chat_widget.add_error_message(
                "This remote prompt contains local image paths or command syntax that cannot be restored safely. Write a new message and reattach any images.".into(),
            );
            tui.frame_requester().schedule_frame();
            return Ok(AppRunControl::Continue);
        }
        if self.pending_server_profiles.contains_key(&thread_id) {
            self.chat_widget.restore_user_message_to_composer(prompt);
            self.chat_widget.add_error_message(
                "Wait for permissions to update before editing this prompt.".into(),
            );
            tui.frame_requester().schedule_frame();
            return Ok(AppRunControl::Continue);
        }
        let Some(index) = self
            .transcript_cells
            .iter()
            .position(|cell| Arc::ptr_eq(cell, &selected_cell))
        else {
            self.restore_backtrack_prompt_after_revert_error(
                prompt,
                "the selected prompt is no longer visible",
            );
            tui.frame_requester().schedule_frame();
            return Ok(AppRunControl::Continue);
        };
        let nth_user_message = crate::app_backtrack::user_count(&self.transcript_cells[..index]);
        let selection: Result<(String, Vec<Turn>)> = async {
            let channel = self.thread_event_channels.get(&thread_id).ok_or_else(|| {
                color_eyre::eyre::eyre!("the selected thread is no longer available")
            })?;
            let (start_item, loaded_tail, latest_turn_id) = {
                let store = channel.store.lock().await;
                (
                    store.turns.iter().find_map(|turn| {
                        turn.items
                            .first()
                            .map(|item| (turn.id.clone(), item.id().to_string()))
                    }),
                    store.turns.last().map(|turn| turn.id.clone()),
                    store.latest_turn_id.clone(),
                )
            };
            let mut thread = app_server
                .thread_read(thread_id, /*include_turns*/ false)
                .await?;
            app_server
                .hydrate_initial_thread_history(
                    &mut thread,
                    /*turn_cursor*/ None,
                    /*item_cursor*/ None,
                    /*config*/ None,
                    /*local_settings*/ None,
                    start_item
                        .as_ref()
                        .map(|(turn_id, _)| turn_id)
                        .or(loaded_tail.as_ref())
                        .map_or(
                            crate::app_server_session::HistoryHydrationScope::Complete,
                            |turn_id| {
                                crate::app_server_session::HistoryHydrationScope::ThroughTurn(
                                    turn_id,
                                )
                            },
                        ),
                )
                .await?;
            if thread.turns.last().map(|turn| &turn.id) != latest_turn_id.as_ref() {
                color_eyre::eyre::bail!(
                    "thread history changed; reload the session before editing this prompt"
                );
            }
            // With no retained visible items, the next prompt follows the metadata-only tail.
            let start_item = start_item.or_else(|| {
                loaded_tail.as_ref().and_then(|tail| {
                    thread
                        .turns
                        .iter()
                        .position(|turn| &turn.id == tail)
                        .and_then(|index| {
                            thread.turns[index + 1..].iter().find_map(|turn| {
                                turn.items
                                    .first()
                                    .map(|item| (turn.id.clone(), item.id().to_string()))
                            })
                        })
                })
            });
            let before_turn_id = crate::app_backtrack::backtrack_revert_before_turn_id(
                &thread.turns,
                start_item.as_ref(),
                nth_user_message,
                &mut prompt,
            )?;
            // Keep the store aligned with the displayed prefix, including retained live turns.
            let cut = thread
                .turns
                .iter()
                .position(|turn| turn.id == before_turn_id)
                .ok_or_else(|| color_eyre::eyre::eyre!("selected turn disappeared"))?;
            thread.turns.truncate(cut);
            if let Some((turn_id, item_id)) = &start_item {
                for turn in &mut thread.turns {
                    if &turn.id == turn_id {
                        if let Some(index) = turn.items.iter().position(|item| item.id() == item_id)
                        {
                            turn.items.drain(..index);
                        }
                        break;
                    }
                    turn.items.clear();
                }
            }
            Ok((before_turn_id, thread.turns))
        }
        .await;
        let (before_turn_id, retained_turns) = match selection {
            Ok(selection) => selection,
            Err(err) => {
                self.restore_backtrack_prompt_after_revert_error(prompt, err);
                tui.frame_requester().schedule_frame();
                return Ok(AppRunControl::Continue);
            }
        };
        if mode == PromptEditMode::Fork {
            self.session_telemetry.counter(
                "codex.thread.fork",
                /*inc*/ 1,
                &[("source", "transcript")],
            );
            self.refresh_in_memory_config_from_disk_best_effort("forking the thread")
                .await;
            let mut config = self.config.clone();
            if app_server.uses_remote_workspace() {
                config
                    .workspace_roots
                    .clone_from(&self.chat_widget.config_ref().workspace_roots);
            }
            config.model = Some(self.chat_widget.current_model().to_string());
            config.model_reasoning_effort = self.chat_widget.current_reasoning_effort();
            let selected_profile = self.confirmed_server_profile(thread_id);
            let started = if retained_turns.is_empty() && !app_server.has_older_history(thread_id) {
                app_server
                    .start_thread_with_session_start_source(
                        &self.local_settings,
                        &config,
                        /*session_start_source*/ None,
                        /*remote_cwd_override*/ None,
                        selected_profile.as_ref(),
                    )
                    .await
            } else {
                app_server
                    .fork_thread_at(
                        &self.local_settings,
                        config,
                        thread_id,
                        /*last_turn_id*/ None,
                        Some(before_turn_id),
                        ForkGoalContinuation::StartIfIdle,
                        selected_profile.as_ref(),
                    )
                    .await
            };
            match started {
                Ok(forked) => {
                    self.detach_current_thread_for_navigation(
                        app_server,
                        Some(forked.session.thread_id),
                    )
                    .await;
                    match self
                        .replace_chat_widget_with_app_server_thread(
                            tui,
                            forked,
                            ThreadAttachPresentation::PromptEdit,
                            /*initial_user_message*/ None,
                        )
                        .await
                    {
                        Ok(()) => self.chat_widget.restore_user_message_to_composer(prompt),
                        Err(err) => self.restore_backtrack_prompt_after_revert_error(prompt, err),
                    }
                }
                Err(err) => self.restore_backtrack_prompt_after_revert_error(prompt, err),
            }
            tui.frame_requester().schedule_frame();
            return Ok(AppRunControl::Continue);
        }
        let reverted = match app_server
            .revert_thread(thread_id, before_turn_id, &retained_turns)
            .await
        {
            Ok(reverted) => reverted,
            Err(err) => {
                // Validation and unsupported-method errors leave history unchanged. Other
                // failures can arrive after the server has already committed the revert.
                if matches!(&err, codex_app_server_client::TypedRequestError::Server { source, .. }
                if matches!(source.code, -32602..=-32600))
                {
                    self.restore_backtrack_prompt_after_revert_error(prompt, err);
                    tui.frame_requester().schedule_frame();
                    return Ok(AppRunControl::Continue);
                }
                self.chat_widget.restore_user_message_to_composer(prompt);
                return Err(color_eyre::Report::new(err).wrap_err(
                    "prompt edit could not be confirmed; resume this session to reload its history",
                ));
            }
        };
        self.chat_widget
            .restore_user_message_to_composer(prompt.clone());
        // Stop on any post-mutation failure: accepting input against the old displayed
        // transcript would hide the fact that server history has already changed.
        tokio::time::timeout(Duration::from_secs(/*secs*/ 10), async {
            while let Some(event) = app_server.next_event().await {
                let is_reverted = matches!(
                    &event,
                    AppServerEvent::ServerNotification(notification)
                        if matches!(notification.as_ref(), ServerNotification::ThreadReverted(notification)
                            if notification.thread_id == thread_id.to_string())
                );
                self.handle_app_server_event(app_server, event).await;
                if is_reverted {
                    return Ok(());
                }
            }
            Err(color_eyre::eyre::eyre!("app-server disconnected"))
        }).await
            .wrap_err("history was reverted, but refreshing the session timed out; resume this session to reload it")??;
        // Preserve the widget and unrelated threads; only replace this replay store.
        self.chat_widget.restore_thread_input_state(
            /*input_state*/ None,
            crate::chatwidget::ThreadInputStateRestoreMode {
                preserve_in_flight_turn: false,
            },
        );
        self.chat_widget.restore_user_message_to_composer(prompt);
        self.chat_widget
            .set_queue_autosend_suppressed(/*suppressed*/ true);
        self.abort_thread_event_listener(thread_id);
        if let Some(mut rx) = self.active_thread_rx.take() {
            while let Ok(event) = rx.try_recv() {
                if let ThreadBufferedEvent::Notification(notification) = event {
                    self.chat_widget.handle_server_notification(
                        *notification,
                        Some(ReplayKind::ThreadSnapshot),
                    );
                }
            }
        }
        let mut session = self
            .thread_event_channels
            .get(&thread_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("reverted thread is no longer available"))?
            .store
            .lock()
            .await
            .session
            .clone()
            .ok_or_else(|| color_eyre::eyre::eyre!("reverted thread has no session"))?;
        session.rollout_path = reverted.thread.path.clone();
        if self.primary_thread_id == Some(thread_id) {
            self.primary_session_configured = Some(session.clone());
        }
        self.thread_event_channels.remove(&thread_id);
        self.active_thread_id = None;
        self.recap.reset_for_new_thread(Instant::now());
        self.recap.seed_from_turns(&retained_turns, Instant::now());
        self.retain_realtime_replay_state_before_replace();
        self.forget_realtime_replay_thread(thread_id);
        self.chat_widget
            .reset_after_prompt_revert(reverted.thread.path, &retained_turns);
        self.ensure_thread_channel(thread_id)
            .store
            .lock()
            .await
            .set_session(session, retained_turns);
        self.activate_thread_channel(thread_id).await;
        // Apply the trim after any transcript inserts produced by shutdown notifications.
        // The existing reset barrier keeps terminal input blocked until then.
        self.pending_thread_switch_resets += 1;
        self.app_event_tx.send(AppEvent::FinishPromptRevert {
            thread_id,
            nth_user_message,
        });
        tui.frame_requester().schedule_frame();
        Ok(AppRunControl::Continue)
    }
}
