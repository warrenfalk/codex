use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::EventFirehoseResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadSortKey;
use codex_app_server_protocol::ThreadSourceKind;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_protocol::protocol::SubAgentSource;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io;
use std::time::Duration;

pub(super) struct Observer<'a> {
    pub(super) client: AppServerClient,
    pub(super) threads: &'a mut BTreeMap<String, Thread>,
    pub(super) dirty: BTreeSet<String>,
    pub(super) removed: BTreeSet<String>,
    pub(super) membership: BTreeMap<String, Membership>,
}

pub(super) enum Membership {
    Present,
    Removed,
}

impl Observer<'_> {
    pub(super) async fn receive(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.client.next_event().await;
            if self.observe(event)? {
                return Ok(());
            }
        }
    }

    fn observe(&mut self, event: Option<AppServerEvent>) -> anyhow::Result<bool> {
        let notification = match event {
            Some(AppServerEvent::ServerNotification(notification)) => notification,
            Some(AppServerEvent::ServerRequest(_)) => {
                anyhow::bail!("passive observer received an interactive request")
            }
            Some(AppServerEvent::Lagged { .. }) | None => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "agents event stream lost synchronization",
                )
                .into());
            }
            Some(AppServerEvent::Disconnected { message }) => {
                return Err(io::Error::new(io::ErrorKind::ConnectionReset, message).into());
            }
        };
        let id = match *notification {
            ServerNotification::ThreadStarted(mut event) => {
                if event.thread.ephemeral {
                    return Ok(false);
                }
                self.removed.remove(&event.thread.id);
                self.membership
                    .insert(event.thread.id.clone(), Membership::Present);
                let id = event.thread.id.clone();
                event.thread.turns.clear();
                self.threads.insert(id.clone(), event.thread);
                id
            }
            ServerNotification::ThreadArchived(event) => {
                self.dirty.remove(&event.thread_id);
                self.membership
                    .insert(event.thread_id.clone(), Membership::Removed);
                self.removed.insert(event.thread_id);
                return Ok(true);
            }
            ServerNotification::ThreadDeleted(event) => {
                self.dirty.remove(&event.thread_id);
                self.membership
                    .insert(event.thread_id.clone(), Membership::Removed);
                self.removed.insert(event.thread_id);
                return Ok(true);
            }
            ServerNotification::ThreadUnarchived(event) => {
                self.removed.remove(&event.thread_id);
                self.membership
                    .insert(event.thread_id.clone(), Membership::Present);
                event.thread_id
            }
            ServerNotification::ThreadClosed(event) => event.thread_id,
            ServerNotification::ThreadStatusChanged(event) => event.thread_id,
            ServerNotification::ThreadNameUpdated(event) => event.thread_id,
            ServerNotification::ThreadSettingsUpdated(event) => event.thread_id,
            ServerNotification::ThreadReverted(event) => event.thread_id,
            ServerNotification::TurnStarted(event) => event.thread_id,
            ServerNotification::TurnCompleted(event) => event.thread_id,
            ServerNotification::ItemStarted(event)
                if matches!(event.item, ThreadItem::UserMessage { .. }) =>
            {
                event.thread_id
            }
            _ => return Ok(false),
        };
        if !self.removed.contains(&id) {
            self.dirty.insert(id);
        }
        Ok(true)
    }

    // Consume notifications while every read is in flight. A change to the thread
    // being read schedules another read, so an older response cannot overwrite it.
    async fn request<T: DeserializeOwned>(&mut self, request: ClientRequest) -> anyhow::Result<T> {
        let handle = self.client.request_handle();
        let response = tokio::time::timeout(Duration::from_secs(30), handle.request_typed(request));
        tokio::pin!(response);
        loop {
            tokio::select! {
                biased;
                event = self.client.next_event() => { self.observe(event)?; },
                result = &mut response => return result.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "agents snapshot read timed out"))?.map_err(Into::into),
            }
        }
    }

    pub(super) async fn seed(&mut self) -> anyhow::Result<()> {
        self.request::<EventFirehoseResponse>(ClientRequest::EventFirehose {
            request_id: request_id(),
            params: None,
        })
        .await?;
        self.dirty.extend(self.threads.keys().cloned());
        // Reconcile archives that happened while disconnected, including descendants.
        if !self.threads.is_empty() {
            for sources in [
                Vec::new(),
                vec![
                    ThreadSourceKind::Exec,
                    ThreadSourceKind::AppServer,
                    ThreadSourceKind::SubAgent,
                    ThreadSourceKind::Unknown,
                ],
            ] {
                let archived = self.list(sources, Listing::Archived).await?;
                self.removed
                    .extend(archived.into_iter().map(|thread| thread.id));
            }
            for (id, membership) in &self.membership {
                if matches!(membership, Membership::Present) {
                    self.removed.remove(id);
                }
            }
        }
        let mut recent = self.list(Vec::new(), Listing::Recent).await?;
        recent.extend(
            self.list(
                vec![ThreadSourceKind::Exec, ThreadSourceKind::AppServer],
                Listing::Recent,
            )
            .await?,
        );
        recent.sort_by(|left, right| {
            right
                .recency_at
                .unwrap_or(right.updated_at)
                .cmp(&left.recency_at.unwrap_or(left.updated_at))
                .then_with(|| right.id.cmp(&left.id))
        });
        for mut thread in recent.into_iter().take(20) {
            self.dirty.insert(thread.id.clone());
            thread.turns.clear();
            self.threads.entry(thread.id.clone()).or_insert(thread);
        }
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        loop {
            let page: ThreadLoadedListResponse = self
                .request(ClientRequest::ThreadLoadedList {
                    request_id: request_id(),
                    params: ThreadLoadedListParams {
                        cursor,
                        limit: Some(100),
                    },
                })
                .await?;
            self.dirty.extend(page.data);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
            anyhow::ensure!(
                seen.insert(cursor.clone()),
                "server repeated a loaded-thread cursor"
            );
        }
        Ok(())
    }

    async fn list(
        &mut self,
        sources: Vec<ThreadSourceKind>,
        listing: Listing,
    ) -> anyhow::Result<Vec<Thread>> {
        let mut data = Vec::new();
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        let mut sort_key = ThreadSortKey::RecencyAt;
        loop {
            let page = self
                .request::<ThreadListResponse>(ClientRequest::ThreadList {
                    request_id: request_id(),
                    params: ThreadListParams {
                        cursor,
                        limit: Some(20),
                        sort_key: Some(sort_key),
                        sort_direction: None,
                        model_providers: Some(Vec::new()),
                        source_kinds: Some(sources.clone()),
                        originators: None,
                        archived: Some(matches!(listing, Listing::Archived)),
                        section_id: None,
                        project_id: None,
                        cwd: None,
                        use_state_db_only: true,
                        search_term: None,
                        parent_thread_id: None,
                        ancestor_thread_id: None,
                    },
                })
                .await;
            let page = match page {
                Err(error)
                    if matches!(error.downcast_ref::<TypedRequestError>(), Some(TypedRequestError::Server { source, .. }) if matches!(source.code, -32600 | -32602) && source.message.contains("recency_at"))
                        && sort_key == ThreadSortKey::RecencyAt =>
                {
                    sort_key = ThreadSortKey::UpdatedAt;
                    cursor = None;
                    data.clear();
                    seen.clear();
                    continue;
                }
                result => result?,
            };
            data.extend(page.data.into_iter().filter(|thread| {
                matches!(listing, Listing::Archived)
                    || (!thread.ephemeral && parent_id(thread).is_none())
            }));
            if matches!(listing, Listing::Recent) && data.len() >= 20 {
                data.truncate(20);
                break;
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
            anyhow::ensure!(
                seen.insert(cursor.clone()),
                "server repeated a thread-list cursor"
            );
        }
        Ok(data)
    }

    pub(super) async fn synchronize(&mut self) -> anyhow::Result<()> {
        while let Some(id) = self.dirty.pop_first() {
            if self.removed.contains(&id) {
                continue;
            }
            let result = self
                .request::<ThreadReadResponse>(ClientRequest::ThreadRead {
                    request_id: request_id(),
                    params: ThreadReadParams {
                        thread_id: id.clone(),
                        include_turns: false,
                    },
                })
                .await;
            if self.removed.contains(&id) {
                continue;
            }
            let read_history = result.is_ok();
            let mut thread = match result {
                Ok(response) => response.thread,
                Err(error) if thread_not_loaded(&error, &id) => {
                    // Unloading is not deletion. Keep metadata observed before the
                    // thread became unreadable, without retrying an unavailable read.
                    let Some(mut thread) = self.threads.get(&id).cloned() else {
                        continue;
                    };
                    thread.status = ThreadStatus::NotLoaded;
                    thread
                }
                Err(error) if matches!(error.downcast_ref::<TypedRequestError>(), Some(TypedRequestError::Server { source, .. }) if source.message == format!("no rollout found for thread id {id}") || source.message == format!("thread not found: {id}")) =>
                {
                    self.removed.insert(id);
                    continue;
                }
                Err(error) => return Err(error),
            };
            if thread.ephemeral {
                self.removed.insert(id);
                continue;
            }
            if read_history && parent_id(&thread).is_none() {
                let turns = self
                    .request::<ThreadTurnsListResponse>(ClientRequest::ThreadTurnsList {
                        request_id: request_id(),
                        params: ThreadTurnsListParams {
                            thread_id: id.clone(),
                            cursor: None,
                            limit: Some(1),
                            sort_direction: None,
                            items_view: None,
                        },
                    })
                    .await;
                if self.removed.contains(&id) {
                    continue;
                }
                match turns {
                    Ok(turns) => {
                        if let Some(turn) = turns.data.first() {
                            crate::agents_list::update_preview(&mut thread, turn);
                        }
                    }
                    Err(error) if thread_not_loaded(&error, &id) => {
                        if let Some(previous) = self.threads.get(&id) {
                            thread.preview.clone_from(&previous.preview);
                        }
                        thread.status = ThreadStatus::NotLoaded;
                    }
                    Err(error) if matches!(error.downcast_ref::<TypedRequestError>(), Some(TypedRequestError::Server { source, .. }) if source.message == format!("thread {id} is not materialized yet; thread/turns/list is unavailable before first user message")) =>
                        {}
                    Err(error) => return Err(error),
                }
            }
            if let Some(parent) = parent_id(&thread)
                && !self.threads.contains_key(&parent)
                && !self.removed.contains(&parent)
            {
                self.dirty.insert(parent);
            }
            thread.turns.clear();
            self.threads.insert(id, thread);
        }
        // Keep descendant metadata even while an archived parent hides its row.
        // If the parent returns, its still-observed children belong to it again.
        self.threads.retain(|id, _| !self.removed.contains(id));
        Ok(())
    }
}

fn thread_not_loaded(error: &anyhow::Error, id: &str) -> bool {
    matches!(error.downcast_ref::<TypedRequestError>(),
        Some(TypedRequestError::Server { source, .. })
            if source.code == -32600 && source.message == format!("thread not loaded: {id}"))
}

enum Listing {
    Recent,
    Archived,
}

fn request_id() -> RequestId {
    RequestId::String(uuid::Uuid::new_v4().to_string())
}

pub(super) fn parent_id(thread: &Thread) -> Option<String> {
    thread
        .parent_thread_id
        .clone()
        .or_else(|| match &thread.source {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id, ..
            }) => Some(parent_thread_id.to_string()),
            _ => None,
        })
}
