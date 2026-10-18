//! Load personal notes separately from the visible transcript.

use super::*;
use crate::app_server_session::HISTORY_ITEM_PAGE_LIMIT;
use crate::app_server_session::thread_items_page_params;
use crate::notes::NoteToSelf;
use crate::pager_overlay::NotesOverlay;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnItemsView;
use std::collections::HashSet;
use uuid::Uuid;

impl App {
    pub(super) fn open_notes(&mut self, tui: &mut tui::Tui) {
        if self.chat_widget.notes.error.is_some() {
            self.chat_widget.reload_notes();
        }
        let _ = tui.enter_alt_screen();
        self.overlay = Some(Overlay::Notes(NotesOverlay::new(
            &self.chat_widget.notes,
            self.keymap.pager.clone(),
        )));
        tui.frame_requester().schedule_frame();
    }

    pub(super) fn load_notes(
        &mut self,
        app_server: &AppServerSession,
        thread_id: ThreadId,
        generation: Uuid,
    ) {
        if self.chat_widget.thread_id() != Some(thread_id)
            || self.chat_widget.notes.generation != generation
        {
            return;
        }
        let request_handle = app_server.request_handle();
        let app_event_tx = self.app_event_tx.clone();
        let task = tokio::spawn(async move {
            let result = read_notes(request_handle, thread_id)
                .await
                .map_err(|error| {
                    tracing::warn!(%thread_id, %generation, error = %error, "failed to load notes to self");
                    error.to_string()
                });
            app_event_tx.send(AppEvent::NotesLoaded {
                thread_id,
                generation,
                result,
            });
        });
        self.chat_widget.notes.task = Some(task.abort_handle());
    }
}

async fn read_notes(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
) -> Result<Vec<NoteToSelf>> {
    let request_id = || RequestId::String(format!("notes-{}", Uuid::new_v4()));
    let response = request_handle
        .request_typed::<ThreadReadResponse>(ClientRequest::ThreadRead {
            request_id: request_id(),
            params: ThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: false,
            },
        })
        .await?;
    // Ephemeral conversations have no saved history; their live notes are retained by the widget.
    if response.thread.ephemeral {
        return Ok(Vec::new());
    }

    let mut notes = Vec::new();
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();
    if response.thread.history_mode == ThreadHistoryMode::Paginated {
        loop {
            let first_page = cursor.is_none();
            let page = request_handle
                .request_typed::<ThreadItemsListResponse>(ClientRequest::ThreadItemsList {
                    request_id: request_id(),
                    params: thread_items_page_params(
                        thread_id,
                        /*turn_id*/ None,
                        cursor,
                        HISTORY_ITEM_PAGE_LIMIT,
                    ),
                })
                .await;
            let page = match page {
                Ok(page) => page,
                Err(error) => {
                    // A fresh thread has no index until its first persisted input. Ask the turn
                    // endpoint to distinguish that empty history from a paging failure, without
                    // materializing an otherwise empty session through a full-history read.
                    if first_page
                        && matches!(&error, TypedRequestError::Server { source, .. } if source.code == -32601)
                    {
                        let probe = request_handle
                            .request_typed::<ThreadTurnsListResponse>(
                                ClientRequest::ThreadTurnsList {
                                    request_id: request_id(),
                                    params: ThreadTurnsListParams {
                                        thread_id: thread_id.to_string(),
                                        cursor: None,
                                        limit: Some(1),
                                        sort_direction: None,
                                        items_view: Some(TurnItemsView::NotLoaded),
                                    },
                                },
                            )
                            .await;
                        let empty = match probe {
                            Ok(page) => page.data.is_empty() && page.next_cursor.is_none(),
                            Err(TypedRequestError::Server { source, .. }) => {
                                source.message.ends_with(
                                    "thread/turns/list is unavailable before first user message",
                                )
                            }
                            Err(_) => false,
                        };
                        if empty {
                            return Ok(Vec::new());
                        }
                    }
                    return Err(error.into());
                }
            };
            notes.extend(page.data.into_iter().filter_map(|entry| match entry.item {
                ThreadItem::NoteToSelf { id, note } => Some(NoteToSelf { id, text: note }),
                _ => None,
            }));
            let Some(next) = page.next_cursor else {
                return Ok(notes);
            };
            if !seen_cursors.insert(next.clone()) {
                color_eyre::eyre::bail!(
                    "Notes could not be loaded: history returned a repeated cursor"
                );
            }
            cursor = Some(next);
        }
    }

    let response = request_handle
        .request_typed::<ThreadReadResponse>(ClientRequest::ThreadRead {
            request_id: request_id(),
            params: ThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: true,
            },
        })
        .await;
    let response = match response {
        Ok(response) => response,
        Err(TypedRequestError::Server { source, .. })
            if source
                .message
                .ends_with("includeTurns is unavailable before first user message") =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.into()),
    };
    Ok(response
        .thread
        .turns
        .into_iter()
        .rev()
        .flat_map(|turn| turn.items.into_iter().rev())
        .filter_map(|item| match item {
            ThreadItem::NoteToSelf { id, note } => Some(NoteToSelf { id, text: note }),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;
