use super::refresh::request;
use crate::agents_list::parent_id;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadSortKey;
use codex_app_server_protocol::ThreadSourceKind;
use std::collections::HashSet;
use uuid::Uuid;

pub(super) enum Listing {
    Recent,
    Archived,
}

pub(super) async fn loaded(handle: &AppServerRequestHandle) -> anyhow::Result<Vec<String>> {
    let mut ids = Vec::new();
    let mut cursor = None;
    let mut seen = HashSet::new();
    loop {
        let page: ThreadLoadedListResponse = request(
            handle,
            ClientRequest::ThreadLoadedList {
                request_id: RequestId::String(Uuid::new_v4().to_string()),
                params: ThreadLoadedListParams {
                    cursor,
                    limit: Some(100),
                },
            },
        )
        .await?;
        ids.extend(page.data);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(ids);
        }
        anyhow::ensure!(
            seen.insert(cursor.clone()),
            "server repeated a loaded-thread cursor"
        );
    }
}

pub(super) async fn list(
    handle: &AppServerRequestHandle,
    sources: Vec<ThreadSourceKind>,
    listing: Listing,
) -> anyhow::Result<Vec<Thread>> {
    let mut data = Vec::new();
    let mut cursor = None;
    let mut seen = HashSet::new();
    let mut sort_key = ThreadSortKey::RecencyAt;
    loop {
        let page = request::<ThreadListResponse>(
            handle,
            ClientRequest::ThreadList {
                request_id: RequestId::String(Uuid::new_v4().to_string()),
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
            },
        )
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
            return Ok(data);
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(data);
        }
        anyhow::ensure!(
            seen.insert(cursor.clone()),
            "server repeated a thread-list cursor"
        );
    }
}
