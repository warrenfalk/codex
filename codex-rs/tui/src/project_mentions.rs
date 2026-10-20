//! Explicit project attachments and bounded discovery on the connected server.

use std::collections::HashSet;
use std::time::Duration;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::FsGetMetadataParams;
use codex_app_server_protocol::FsGetMetadataResponse;
use codex_app_server_protocol::FsReadDirectoryParams;
use codex_app_server_protocol::FsReadDirectoryResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadSortKey;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

pub(crate) const MAX_PROJECTS: usize = 100;
const PREFIX: &str = "project://";

pub(crate) fn project_target(path: &str) -> String {
    format!("{PREFIX}{}", urlencoding::encode(path).replace("%2F", "/"))
}

pub(crate) fn project_path(target: &str) -> Option<AbsolutePathBuf> {
    let path = urlencoding::decode(target.strip_prefix(PREFIX)?).ok()?;
    AbsolutePathBuf::from_absolute_path_checked(path.as_ref()).ok()
}

pub(crate) fn project_name(path: &str) -> String {
    let basename = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    let name: String = basename
        .chars()
        .take(80)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    // The prefix also keeps environment-variable names out of mention history heuristics.
    format!("project-{}", if name.is_empty() { "root" } else { &name })
}

pub(crate) fn is_path_query(query: &str) -> bool {
    query.starts_with(['/', '\\', '.']) || query.as_bytes().get(1) == Some(&b':')
}

pub(crate) fn selected_projects(items: &[UserInput]) -> Vec<AbsolutePathBuf> {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter_map(|item| match item {
            UserInput::Mention { path, .. } => project_path(path),
            _ => None,
        })
        .filter(|path| seen.insert(path.clone()))
        .take(MAX_PROJECTS)
        .collect()
}

pub(crate) fn spawn_search(
    handle: AppServerRequestHandle,
    thread_id: ThreadId,
    query: String,
    cwd: AbsolutePathBuf,
    app_event_tx: AppEventSender,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(/*millis*/ 100)).await;
        let result = tokio::time::timeout(Duration::from_secs(/*secs*/ 10), async {
            if is_path_query(&query) {
                directory_candidates(&handle, &query, &cwd).await
            } else {
                recent_projects(&handle).await
            }
        })
        .await;
        app_event_tx.send(AppEvent::ProjectSearchResult {
            thread_id,
            query,
            result: result
                .map_err(|_| "Project discovery timed out".to_string())
                .and_then(|result| result.map_err(|err| err.to_string())),
        });
    })
}

async fn recent_projects(handle: &AppServerRequestHandle) -> anyhow::Result<Vec<AbsolutePathBuf>> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = None;
    // Keep discovery bounded even when hundreds of recent sessions share one directory.
    for page in 0..3 {
        let response: ThreadListResponse = handle
            .request_typed(ClientRequest::ThreadList {
                request_id: RequestId::String(format!(
                    "project-list-{}-{page}",
                    uuid::Uuid::new_v4()
                )),
                params: ThreadListParams {
                    cursor,
                    limit: Some(100),
                    sort_key: Some(ThreadSortKey::UpdatedAt),
                    sort_direction: Some(SortDirection::Desc),
                    model_providers: Some(Vec::new()),
                    source_kinds: Some(Vec::new()),
                    use_state_db_only: true,
                    excluded_thread_ids: None,
                    originators: None,
                    archived: Some(false),
                    section_id: None,
                    project_id: None,
                    cwd: None,
                    search_term: None,
                    parent_thread_id: None,
                    ancestor_thread_id: None,
                },
            })
            .await?;
        for thread in response.data {
            if !thread.ephemeral && seen.insert(thread.cwd.clone()) {
                roots.push(thread.cwd);
                if roots.len() == MAX_PROJECTS {
                    return Ok(roots);
                }
            }
        }
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(roots)
}

async fn directory_candidates(
    handle: &AppServerRequestHandle,
    query: &str,
    cwd: &AbsolutePathBuf,
) -> anyhow::Result<Vec<AbsolutePathBuf>> {
    let path = AbsolutePathBuf::resolve_path_against_base(query, cwd);
    let metadata = handle
        .request_typed::<FsGetMetadataResponse>(ClientRequest::FsGetMetadata {
            request_id: RequestId::String(format!("project-stat-{}", uuid::Uuid::new_v4())),
            params: FsGetMetadataParams { path: path.clone() },
        })
        .await;
    let exact_directory = metadata.is_ok_and(|metadata| metadata.is_directory);
    let (parent, prefix) = if exact_directory {
        (path.clone(), String::new())
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Directory has no parent"))?;
        (
            AbsolutePathBuf::from_absolute_path(parent)?,
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        )
    };
    let response: FsReadDirectoryResponse = handle
        .request_typed(ClientRequest::FsReadDirectory {
            request_id: RequestId::String(format!("project-directory-{}", uuid::Uuid::new_v4())),
            params: FsReadDirectoryParams {
                path: parent.clone(),
            },
        })
        .await?;
    let mut roots = response
        .entries
        .into_iter()
        .filter(|entry| entry.is_directory && entry.file_name.starts_with(&prefix))
        .map(|entry| parent.join(entry.file_name))
        .collect::<Vec<_>>();
    roots.sort();
    if exact_directory {
        roots.insert(0, path);
    }
    roots.truncate(MAX_PROJECTS);
    Ok(roots)
}

/// Verify selections again before starting work; stale history must not attach missing paths.
pub(crate) async fn validate_projects(
    handle: &AppServerRequestHandle,
    paths: &[AbsolutePathBuf],
) -> anyhow::Result<()> {
    for path in paths {
        let metadata: FsGetMetadataResponse = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 10),
            handle.request_typed(ClientRequest::FsGetMetadata {
                request_id: RequestId::String(format!("project-validate-{}", uuid::Uuid::new_v4())),
                params: FsGetMetadataParams { path: path.clone() },
            }),
        )
        .await??;
        anyhow::ensure!(
            metadata.is_directory,
            "Project is not a directory: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "project_mentions_tests.rs"]
mod tests;
