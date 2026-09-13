use super::AgentsJsonOptions;
use crate::agents_list::AgentsOverviewGroup;
use crate::agents_list::AgentsOverviewProjectGroup;
use crate::agents_list::display_title;
use crate::agents_list::parent_id;
use crate::agents_model::AgentsModel;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadActiveFlag;
use codex_app_server_protocol::ThreadStatus;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

pub(super) fn snapshot(
    model: &AgentsModel,
    options: &AgentsJsonOptions,
) -> anyhow::Result<serde_json::Value> {
    let threads: BTreeMap<_, _> = model
        .threads
        .values()
        .flatten()
        .map(|thread| (thread.id.clone(), thread))
        .collect();
    let mut families = BTreeMap::<String, Vec<&Thread>>::new();
    for thread in threads.values().copied() {
        let mut root = thread;
        let mut seen = BTreeSet::from([&root.id]);
        while let Some(parent) = parent_id(root) {
            let Some(ancestor) = threads.get(&parent) else {
                break;
            };
            anyhow::ensure!(seen.insert(&ancestor.id), "cyclic agents ancestry");
            root = ancestor;
        }
        // Never invent a root ID for an orphaned subagent.
        if parent_id(root).is_none() {
            families.entry(root.id.clone()).or_default().push(thread);
        }
    }
    let mut sessions = Vec::new();
    for (id, family) in families {
        let root = threads[&id];
        let status = family
            .iter()
            .map(|thread| AgentsOverviewGroup::for_status(&thread.status))
            .min()
            .unwrap_or(AgentsOverviewGroup::Finished);
        let working = family.iter().any(|thread| matches!(&thread.status, ThreadStatus::Active { active_flags } if active_flags.is_empty()));
        let mut attention = Vec::new();
        for (reason, flag) in [
            ("approval", ThreadActiveFlag::WaitingOnApproval),
            ("userInput", ThreadActiveFlag::WaitingOnUserInput),
        ] {
            if family.iter().any(|thread| matches!(&thread.status, ThreadStatus::Active { active_flags } if active_flags.contains(&flag))) { attention.push(reason); }
        }
        if family
            .iter()
            .any(|thread| matches!(thread.status, ThreadStatus::SystemError))
        {
            attention.push("error");
        }
        sessions.push(serde_json::json!({
            "id": id, "title": display_title(root), "cwd": root.cwd,
            "name": root.name, "preview": root.preview, "createdAt": root.created_at,
            "updatedAt": root.updated_at, "recencyAt": root.recency_at, "gitInfo": root.git_info,
            "status": status, "rootStatus": root.status,
            "project": AgentsOverviewProjectGroup::for_thread(root, options.worktree_grouping),
            "loaded": family.iter().any(|thread| !matches!(thread.status, ThreadStatus::NotLoaded)),
            "working": working, "attention": attention,
        }));
    }
    Ok(serde_json::json!({
        "version": 1, "connection": "connected",
        "counts": { "total": sessions.len(), "working": sessions.iter().filter(|session| session["working"] == true).count(), "needsAttention": sessions.iter().filter(|session| session["attention"].as_array().is_some_and(|attention| !attention.is_empty())).count() },
        "sessions": sessions,
    }))
}
