//! Choosing and applying edits to earlier prompts without changing their attachments.

use super::app_server_event_targets::ServerNotificationThreadTarget;
use super::app_server_event_targets::server_notification_thread_target;
use super::app_server_event_targets::server_request_thread_id;
use super::session_lifecycle::ThreadAttachPresentation;
use super::*;
use crate::app_backtrack::BacktrackSelection;
use crate::app_server_session::ForkGoalContinuation;
use crate::app_server_session::ResumeModelSettings;
use crate::bottom_pane::SelectionDescriptionLayout;
use crate::bottom_pane::popup_consts::standard_popup_hint_line_for_keymap;
use crate::chatwidget::UserMessage;
use codex_app_server_client::AppServerEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptEditMode {
    Replace,
    Fork,
}

impl App {
    pub(crate) fn show_prompt_edit_menu(&mut self, selection: BacktrackSelection) {
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
            SelectionItem {
                name: name.to_string(),
                description: Some(description.to_string()),
                actions: vec![Box::new(move |tx| {
                    let BacktrackSelection {
                        thread_id,
                        nth_user_message,
                        prompt,
                    } = selection.clone();
                    tx.send(match mode {
                        PromptEditMode::Replace => AppEvent::RevertSessionForPromptEdit {
                            thread_id,
                            nth_user_message,
                            prompt,
                        },
                        PromptEditMode::Fork => AppEvent::ForkSessionForPromptEdit {
                            thread_id,
                            nth_user_message,
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
            footer_hint: Some(standard_popup_hint_line_for_keymap(&self.keymap.list)),
            items,
            description_layout: SelectionDescriptionLayout::StackBelowWhenNarrow {
                min_description_width: 28,
            },
            ..Default::default()
        });
    }

    pub(super) async fn edit_previous_prompt(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        selection: BacktrackSelection,
        mode: PromptEditMode,
    ) -> Result<AppRunControl> {
        let BacktrackSelection {
            thread_id,
            nth_user_message,
            mut prompt,
        } = selection;
        if self.chat_widget.thread_id() != Some(thread_id) {
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
        let turns = match self.thread_event_channels.get(&thread_id) {
            Some(channel) => {
                let store = channel.store.lock().await;
                let mut turns = store.turns.clone();
                // Snapshot turns contain loaded history; newer live turns remain in
                // the replay buffer and must also be visible to prompt-edit lookups.
                for event in &store.buffer {
                    let ThreadBufferedEvent::Notification(notification) = event else {
                        continue;
                    };
                    match notification.as_ref() {
                        ServerNotification::TurnStarted(notification)
                            if !turns.iter().any(|turn| turn.id == notification.turn.id) =>
                        {
                            turns.push(notification.turn.clone());
                        }
                        ServerNotification::ItemCompleted(notification) => {
                            if matches!(
                                notification.item,
                                ThreadItem::UserMessage { .. }
                                    | ThreadItem::EnteredReviewMode { .. }
                                    | ThreadItem::ExitedReviewMode { .. }
                            ) && let Some(turn) = turns
                                .iter_mut()
                                .find(|turn| turn.id == notification.turn_id)
                                && !turn
                                    .items
                                    .iter()
                                    .any(|item| item.id() == notification.item.id())
                            {
                                turn.items.push(notification.item.clone());
                            }
                        }
                        ServerNotification::TurnCompleted(notification) => {
                            if let Some(turn) = turns
                                .iter_mut()
                                .find(|turn| turn.id == notification.turn.id)
                            {
                                turn.status = notification.turn.status.clone();
                                turn.error = notification.turn.error.clone();
                                turn.started_at = notification.turn.started_at;
                                turn.completed_at = notification.turn.completed_at;
                                turn.duration_ms = notification.turn.duration_ms;
                            }
                        }
                        _ => {}
                    }
                }
                Some(turns)
            }
            None => None,
        };
        if mode == PromptEditMode::Replace {
            let result = async {
                let turns = turns.ok_or_else(|| {
                    color_eyre::eyre::eyre!(
                        "the selected thread is no longer available for prompt editing"
                    )
                })?;
                let before_turn_id = crate::app_backtrack::backtrack_fork_before_turn_id(
                    &turns,
                    nth_user_message,
                    &mut prompt,
                )?
                .or_else(|| turns.first().map(|turn| turn.id.clone()))
                .ok_or_else(|| color_eyre::eyre::eyre!("the selected prompt was not found"))?;
                app_server
                    .revert_thread_before(thread_id, before_turn_id)
                    .await
            }
            .await;
            match result {
                Ok(_) => {
                    // Old notifications queued before the completed RPC must not restore discarded
                    // turns. Keep delivering events belonging to other conversations.
                    while let Some(event) = tokio::select! {
                        biased;
                        event = app_server.next_event() => event,
                        _ = std::future::ready(()) => None,
                    } {
                        let belongs_to_reverted_thread = match &event {
                            AppServerEvent::ServerNotification(notification) => {
                                server_notification_thread_target(notification)
                                    == ServerNotificationThreadTarget::Thread(thread_id)
                            }
                            AppServerEvent::ServerRequest(request) => {
                                server_request_thread_id(request) == Some(thread_id)
                            }
                            AppServerEvent::Lagged { .. } | AppServerEvent::Disconnected { .. } => {
                                false
                            }
                        };
                        if !belongs_to_reverted_thread {
                            self.handle_app_server_event(app_server, event).await;
                        }
                    }
                    // Refresh authoritative settings and startup notifications after draining the
                    // old runtime's events. Resume reuses the loaded thread and its subscription.
                    match app_server
                        .resume_thread(
                            &self.local_settings,
                            self.config.clone(),
                            thread_id,
                            ResumeModelSettings::PreserveExistingThread,
                        )
                        .await
                    {
                        Ok(started) => self.app_event_tx.send(AppEvent::PromptEditReverted {
                            started: Box::new(started),
                            prompt,
                        }),
                        Err(err) => {
                            self.chat_widget.restore_user_message_to_composer(prompt);
                            self.chat_widget.add_error_message(format!(
                                "Conversation was replaced, but could not be refreshed: {err:#}"
                            ));
                        }
                    }
                }
                Err(err) => {
                    self.chat_widget.restore_user_message_to_composer(prompt);
                    self.chat_widget.add_error_message(format!(
                        "Failed to replace the conversation from the selected prompt: {err:#}"
                    ));
                }
            }
            tui.frame_requester().schedule_frame();
            return Ok(AppRunControl::Continue);
        }

        self.session_telemetry.counter(
            "codex.thread.fork",
            /*inc*/ 1,
            &[("source", "transcript")],
        );
        self.refresh_in_memory_config_from_disk_best_effort("forking the thread")
            .await;
        let config = self.fresh_session_config();
        let selected_profile = self.confirmed_server_profile(thread_id);
        let started = match turns {
            Some(turns) => match crate::app_backtrack::backtrack_fork_before_turn_id(
                &turns,
                nth_user_message,
                &mut prompt,
            ) {
                Ok(before_turn_id)
                    if before_turn_id.is_some() || app_server.has_older_history(thread_id) =>
                {
                    let before_turn_id =
                        before_turn_id.or_else(|| turns.first().map(|turn| turn.id.clone()));
                    app_server
                        .fork_thread_at(
                            &self.local_settings,
                            config.clone(),
                            thread_id,
                            /*last_turn_id*/ None,
                            before_turn_id,
                            ForkGoalContinuation::StartIfIdle,
                            selected_profile.as_ref(),
                        )
                        .await
                }
                Ok(_) => {
                    app_server
                        .start_thread_with_session_start_source(
                            &self.local_settings,
                            &config,
                            /*session_start_source*/ None,
                            /*remote_cwd_override*/ None,
                            selected_profile.as_ref(),
                        )
                        .await
                }
                Err(err) => Err(err),
            },
            None => Err(color_eyre::eyre::eyre!(
                "the selected thread is no longer available for prompt editing"
            )),
        };
        match started {
            Ok(forked) => {
                self.shutdown_current_thread(app_server).await;
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
                    Err(err) => {
                        self.restore_backtrack_prompt_after_branch_error(prompt, err);
                    }
                }
            }
            Err(err) => {
                self.restore_backtrack_prompt_after_branch_error(prompt, err);
            }
        }
        tui.frame_requester().schedule_frame();

        Ok(AppRunControl::Continue)
    }

    pub(super) async fn finish_prompt_edit_revert(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &AppServerSession,
        started: AppServerStartedThread,
        prompt: UserMessage,
    ) -> Result<()> {
        let AppServerStartedThread { session, turns, .. } = started;
        let thread_id = session.thread_id;
        self.abort_thread_event_listener(thread_id);
        let channel = ThreadEventChannel::new(THREAD_EVENT_CHANNEL_CAPACITY);
        let snapshot = {
            let mut store = channel.store.lock().await;
            store.set_session(session.clone(), turns);
            store.snapshot()
        };
        self.thread_event_channels.insert(thread_id, channel);
        if self.primary_thread_id == Some(thread_id) {
            self.primary_session_configured = Some(session);
        }
        if self.chat_widget.thread_id() != Some(thread_id) {
            return Ok(());
        }
        self.active_thread_id = None;
        self.active_thread_rx = None;
        self.activate_thread_channel(thread_id).await;
        self.recap = recap::RecapState::default();
        let now = Instant::now();
        self.recap.seed_from_turns(&snapshot.turns, now);
        self.schedule_recap_check(thread_id, now);
        self.render_thread_snapshot(
            tui, app_server, thread_id, snapshot, /*resume_restored_queue*/ false,
        )?;
        self.chat_widget.add_info_message(
            "You’re editing this conversation from this point".to_string(),
            /*hint*/ None,
        );
        self.chat_widget.restore_user_message_to_composer(prompt);
        tui.frame_requester().schedule_frame();
        Ok(())
    }
}
