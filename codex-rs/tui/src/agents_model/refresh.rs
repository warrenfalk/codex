//! Read-only discovery and metadata refresh for both agents presentations.

use super::listing;
use super::listing::Listing;
use crate::agents_list::parent_id;
use crate::agents_list::preview_text;
use crate::agents_list::update_preview;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadSourceKind;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_protocol::ThreadId;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Default)]
pub(crate) struct AgentsRefresh {
    pub(crate) threads: HashMap<ThreadId, Option<Thread>>,
    pub(crate) last_messages: HashMap<ThreadId, String>,
    pub(crate) recent_seed_complete: bool,
    pub(crate) removed: HashSet<ThreadId>,
    /// Keep partial data usable in the dashboard while preventing incomplete JSON snapshots.
    pub(crate) error: Option<anyhow::Error>,
}

pub(crate) struct RefreshRequest {
    pub(crate) id: Uuid,
    pub(super) discover: bool,
    pub(super) reconcile_archives: bool,
    pub(super) threads: HashMap<ThreadId, Option<Thread>>,
    pub(super) known_ids: HashSet<ThreadId>,
}

impl RefreshRequest {
    pub(crate) async fn fetch(
        mut self,
        handle: AppServerRequestHandle,
    ) -> anyhow::Result<AgentsRefresh> {
        let mut refresh = AgentsRefresh {
            recent_seed_complete: true,
            ..Default::default()
        };
        if self.discover {
            if self.reconcile_archives {
                for sources in [
                    Vec::new(),
                    vec![
                        ThreadSourceKind::Exec,
                        ThreadSourceKind::AppServer,
                        ThreadSourceKind::SubAgent,
                        ThreadSourceKind::Unknown,
                    ],
                ] {
                    refresh.removed.extend(
                        listing::list(&handle, sources, Listing::Archived)
                            .await?
                            .into_iter()
                            .filter_map(|thread| ThreadId::from_string(&thread.id).ok()),
                    );
                }
            }
            let (loaded, interactive, non_interactive) = tokio::join!(
                listing::loaded(&handle),
                listing::list(&handle, Vec::new(), Listing::Recent),
                listing::list(
                    &handle,
                    vec![ThreadSourceKind::Exec, ThreadSourceKind::AppServer],
                    Listing::Recent
                ),
            );
            let loaded = loaded?;
            let mut recent = Vec::new();
            for result in [interactive, non_interactive] {
                match result {
                    Ok(threads) => recent.extend(threads),
                    Err(error) => {
                        refresh.recent_seed_complete = false;
                        refresh.error.get_or_insert(error);
                    }
                }
            }
            recent.sort_by(|left, right| {
                right
                    .recency_at
                    .unwrap_or(right.updated_at)
                    .cmp(&left.recency_at.unwrap_or(left.updated_at))
                    .then_with(|| right.id.cmp(&left.id))
            });
            for thread in recent.into_iter().take(20) {
                if let Ok(id) = ThreadId::from_string(&thread.id) {
                    self.threads.entry(id).or_default().get_or_insert(thread);
                }
            }
            for id in loaded
                .into_iter()
                .filter_map(|id| ThreadId::from_string(&id).ok())
            {
                self.threads.entry(id).or_default();
            }
        }
        self.threads.retain(|id, _| !refresh.removed.contains(id));
        let mut seen: HashSet<_> = self.threads.keys().copied().collect();
        while !self.threads.is_empty() {
            let mut pending = std::mem::take(&mut self.threads).into_iter();
            let mut reads = tokio::task::JoinSet::new();
            loop {
                while reads.len() < 16 {
                    let Some((id, previous)) = pending.next() else {
                        break;
                    };
                    let handle = handle.clone();
                    reads.spawn(async move { (id, read_thread(&handle, id, previous).await) });
                }
                let Some(result) = reads.join_next().await else {
                    break;
                };
                let (id, record) = result?;
                if let Some(error) = record.error {
                    refresh.error.get_or_insert(error);
                }
                if record.removed {
                    refresh.removed.insert(id);
                    continue;
                }
                if let Some(thread) = &record.thread
                    && !thread.ephemeral
                    && let Some(parent) = parent_id(thread)
                {
                    let parent = ThreadId::from_string(&parent)?;
                    if !self.known_ids.contains(&parent)
                        && !refresh.removed.contains(&parent)
                        && seen.insert(parent)
                    {
                        self.threads.insert(parent, /*v*/ None);
                    }
                }
                if let Some(message) = record.last_message {
                    refresh.last_messages.insert(id, message);
                }
                refresh.threads.insert(id, record.thread);
            }
        }
        Ok(refresh)
    }
}

#[derive(Default)]
struct ReadRecord {
    thread: Option<Thread>,
    last_message: Option<String>,
    removed: bool,
    error: Option<anyhow::Error>,
}

async fn read_thread(
    handle: &AppServerRequestHandle,
    id: ThreadId,
    previous: Option<Thread>,
) -> ReadRecord {
    let mut thread = match request::<ThreadReadResponse>(
        handle,
        ClientRequest::ThreadRead {
            request_id: RequestId::String(Uuid::new_v4().to_string()),
            params: ThreadReadParams {
                thread_id: id.to_string(),
                include_turns: false,
            },
        },
    )
    .await
    {
        Ok(response) => response.thread,
        Err(error) if thread_not_loaded(&error, id) => {
            return ReadRecord {
                thread: previous.map(|mut thread| {
                    thread.status = ThreadStatus::NotLoaded;
                    thread
                }),
                ..Default::default()
            };
        }
        Err(error) if matches!(error.downcast_ref::<TypedRequestError>(), Some(TypedRequestError::Server { source, .. }) if source.message == format!("no rollout found for thread id {id}") || source.message == format!("thread not found: {id}")) => {
            return ReadRecord {
                removed: true,
                ..Default::default()
            };
        }
        Err(error) => {
            return ReadRecord {
                thread: previous,
                error: Some(error),
                ..Default::default()
            };
        }
    };
    let mut last_message = None;
    let mut read_error = None;
    if !thread.ephemeral {
        match request::<ThreadTurnsListResponse>(
            handle,
            ClientRequest::ThreadTurnsList {
                request_id: RequestId::String(Uuid::new_v4().to_string()),
                params: ThreadTurnsListParams {
                    thread_id: id.to_string(),
                    cursor: None,
                    limit: Some(1),
                    sort_direction: None,
                    items_view: None,
                },
            },
        )
        .await
        {
            Ok(turns) => {
                if let Some(turn) = turns.data.first() {
                    update_preview(&mut thread, turn);
                    last_message = turn.items.iter().rev().find_map(|item| match item {
                        ThreadItem::AgentMessage { text, .. } => Some(preview_text(text)),
                        _ => None,
                    });
                }
            }
            Err(error) if thread_not_loaded(&error, id) => {
                if let Some(previous) = previous {
                    thread.preview = previous.preview;
                }
                thread.status = ThreadStatus::NotLoaded;
            }
            Err(error) if matches!(error.downcast_ref::<TypedRequestError>(), Some(TypedRequestError::Server { source, .. }) if source.message == format!("thread {id} is not materialized yet; thread/turns/list is unavailable before first user message")) =>
                {}
            Err(error) => {
                read_error = Some(error);
            }
        }
    }
    thread.turns.clear();
    ReadRecord {
        thread: Some(thread),
        last_message,
        error: read_error,
        ..Default::default()
    }
}

fn thread_not_loaded(error: &anyhow::Error, id: ThreadId) -> bool {
    matches!(error.downcast_ref::<TypedRequestError>(), Some(TypedRequestError::Server { source, .. }) if source.code == -32600 && source.message == format!("thread not loaded: {id}"))
}

pub(super) async fn request<T: DeserializeOwned>(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
) -> anyhow::Result<T> {
    tokio::time::timeout(
        Duration::from_secs(/*secs*/ 30),
        handle.request_typed(request),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "agents snapshot read timed out"))?
    .map_err(Into::into)
}
