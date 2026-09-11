use std::fs::File;
use std::io::Write;

use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TokenCountEvent;
use codex_protocol::protocol::TokenUsageInfo;
use codex_protocol::protocol::TurnContextItem;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::ResponseItemEnvelope;
use crate::RolloutItem;
use crate::RolloutLine;

#[tokio::test]
async fn reports_activity_from_old_thread_modified_recently() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000001").expect("valid id");
    let path = rollout_path(temp.path(), "2000/01/01", "2000-01-01T00-00-00", thread_id);
    write_rollout(
        &path,
        vec![
            session_meta_line(
                "2000-01-01T00:00:00Z",
                thread_id,
                /*forked_from_id*/ None,
            ),
            turn_context_line("2000-04-01T00:00:00Z", "gpt-5.4"),
            token_count_line("2000-04-01T00:00:00Z", 100, 40, 0, 10, 0),
            token_count_line("2000-05-01T12:00:00Z", 150, 60, 5, 20, 1),
        ],
    )?;
    session_index::append_thread_name(temp.path(), thread_id, "Renamed usage thread").await?;

    let report = generate_usage_report(
        temp.path(),
        UsageReportOptions {
            since: parse_rollout_timestamp("2000-05-01T00:00:00Z").expect("timestamp"),
            until: parse_rollout_timestamp("2000-05-02T00:00:00Z").expect("timestamp"),
        },
    )
    .await?;

    assert_eq!(report.totals.total_tokens, 50);
    assert_eq!(report.totals.input_tokens, 20);
    assert_eq!(report.totals.cached_input_tokens, 5);
    assert_eq!(report.totals.uncached_input_tokens, 15);
    assert_eq!(report.totals.output_tokens, 10);
    assert_eq!(report.totals.reasoning_output_tokens, 1);
    assert_eq!(report.threads.len(), 1);
    assert_eq!(report.threads[0].usage_events, 1);
    assert_eq!(
        report.threads[0].thread_name.as_deref(),
        Some("Renamed usage thread")
    );
    assert_eq!(report.threads[0].display_name, "Renamed usage thread");
    assert_cost_close(report.costs.total_usd, 0.00018875);
    assert_cost_close(report.threads[0].costs.total_usd, 0.00018875);
    assert_cost_close(report.threads[0].costs.reasoning_output_usd, 0.000015);
    Ok(())
}

#[tokio::test]
async fn skips_copied_parent_token_counts_in_forked_rollout() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let parent_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000010").expect("valid id");
    let child_id = ThreadId::from_string("00000000-0000-0000-0000-000000000020").expect("valid id");
    let parent_path = rollout_path(temp.path(), "2000/04/01", "2000-04-01T00-00-00", parent_id);
    let parent_lines = vec![
        session_meta_line(
            "2000-04-01T00:00:00Z",
            parent_id,
            /*forked_from_id*/ None,
        ),
        turn_context_line("2000-04-01T00:00:30Z", "gpt-5.4"),
        token_count_line("2000-04-01T00:01:00Z", 100, 70, 10, 30, 5),
        token_count_line("2000-04-01T00:02:00Z", 200, 130, 20, 70, 15),
    ];
    write_rollout(&parent_path, parent_lines.clone())?;

    let child_path = rollout_path(temp.path(), "2000/05/01", "2000-05-01T00-00-00", child_id);
    let copied_parent_lines = parent_lines
        .into_iter()
        .map(|line| RolloutLine {
            timestamp: "2000-05-01T00:00:00Z".to_string(),
            ordinal: None,
            item: line.item,
        })
        .collect::<Vec<_>>();
    let mut child_lines = vec![session_meta_line(
        "2000-05-01T00:00:00Z",
        child_id,
        Some(parent_id),
    )];
    child_lines.extend(copied_parent_lines);
    child_lines.push(user_prompt_line(
        "2000-05-01T00:02:30Z",
        "Fork follow-up prompt",
    ));
    child_lines.push(token_count_line(
        "2000-05-01T00:03:00Z",
        260,
        170,
        40,
        90,
        20,
    ));
    write_rollout(&child_path, child_lines)?;

    let report = generate_usage_report(
        temp.path(),
        UsageReportOptions {
            since: parse_rollout_timestamp("2000-05-01T00:00:00Z").expect("timestamp"),
            until: parse_rollout_timestamp("2000-05-02T00:00:00Z").expect("timestamp"),
        },
    )
    .await?;

    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(report.totals.total_tokens, 60);
    assert_eq!(report.totals.input_tokens, 40);
    assert_eq!(report.totals.cached_input_tokens, 20);
    assert_eq!(report.totals.uncached_input_tokens, 20);
    assert_eq!(report.totals.output_tokens, 20);
    assert_eq!(report.totals.reasoning_output_tokens, 5);
    assert_eq!(report.threads.len(), 1);
    let child_id_str = child_id.to_string();
    let parent_id_str = parent_id.to_string();
    assert_eq!(
        report.threads[0].thread_id.as_deref(),
        Some(child_id_str.as_str())
    );
    assert_eq!(
        report.threads[0].forked_from_id.as_deref(),
        Some(parent_id_str.as_str())
    );
    assert_eq!(
        report.threads[0].first_prompt.as_deref(),
        Some("Fork follow-up prompt")
    );
    assert_eq!(report.threads[0].display_name, "Fork follow-up prompt");
    Ok(())
}

#[tokio::test]
async fn skips_fork_time_token_counts_when_parent_is_missing() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let parent_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000030").expect("valid id");
    let child_id = ThreadId::from_string("00000000-0000-0000-0000-000000000040").expect("valid id");
    let child_path = rollout_path(temp.path(), "2000/05/01", "2000-05-01T00-00-00", child_id);
    write_rollout(
        &child_path,
        vec![
            session_meta_line("2000-05-01T00:00:00Z", child_id, Some(parent_id)),
            turn_context_line("2000-05-01T00:00:00Z", "gpt-5.4"),
            token_count_line("2000-05-01T00:00:00Z", 200, 130, 20, 70, 15),
            user_prompt_line(
                "2000-05-01T00:02:30Z",
                "This is the first prompt text that should be used when no title exists.",
            ),
            token_count_line("2000-05-01T00:03:00Z", 260, 170, 40, 90, 20),
        ],
    )?;

    let report = generate_usage_report(
        temp.path(),
        UsageReportOptions {
            since: parse_rollout_timestamp("2000-05-01T00:00:00Z").expect("timestamp"),
            until: parse_rollout_timestamp("2000-05-02T00:00:00Z").expect("timestamp"),
        },
    )
    .await?;

    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.totals.total_tokens, 60);
    assert_eq!(report.totals.input_tokens, 40);
    assert_eq!(report.totals.cached_input_tokens, 20);
    assert_eq!(report.totals.uncached_input_tokens, 20);
    assert_eq!(report.totals.output_tokens, 20);
    assert_eq!(report.totals.reasoning_output_tokens, 5);
    assert_eq!(
        report.threads[0].display_name,
        "This is the first prompt text that should be used when no title exists."
    );
    Ok(())
}

fn rollout_path(root: &Path, parts: &str, timestamp: &str, thread_id: ThreadId) -> PathBuf {
    root.join(SESSIONS_SUBDIR)
        .join(parts)
        .join(format!("rollout-{timestamp}-{thread_id}.jsonl"))
}

fn write_rollout(path: &Path, lines: Vec<RolloutLine>) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.parent().expect("rollout parent"))?;
    let mut file = File::create(path)?;
    for line in lines {
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
    }
    Ok(())
}

fn session_meta_line(
    timestamp: &str,
    thread_id: ThreadId,
    forked_from_id: Option<ThreadId>,
) -> RolloutLine {
    RolloutLine {
        timestamp: timestamp.to_string(),
        ordinal: None,
        item: RolloutItem::SessionMeta(SessionMetaLine {
            meta: SessionMeta {
                session_id: thread_id.into(),
                id: thread_id,
                forked_from_id,
                forked_from_ordinal_exclusive: None,
                parent_thread_id: None,
                timestamp: timestamp.to_string(),
                cwd: PathBuf::from("/tmp/project"),
                originator: "test".to_string(),
                cli_version: "test".to_string(),
                source: SessionSource::Cli,
                thread_source: None,
                agent_nickname: None,
                agent_role: None,
                agent_path: None,
                model_provider: Some("openai".to_string()),
                base_instructions: None,
                dynamic_tools: None,
                selected_capability_roots: Vec::new(),
                memory_mode: None,
                history_mode: Default::default(),
                subagent_history_start_ordinal: None,
                history_base: None,
                multi_agent_version: None,
                context_window: None,
            },
            git: None,
        }),
    }
}

fn token_count_line(
    timestamp: &str,
    total_tokens: i64,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
) -> RolloutLine {
    let usage = TokenUsage {
        input_tokens,
        cached_input_tokens,
        cache_write_input_tokens: 0,
        output_tokens,
        reasoning_output_tokens,
        total_tokens,
        codex_rollout_budget_units: None,
    };
    RolloutLine {
        timestamp: timestamp.to_string(),
        ordinal: None,
        item: RolloutItem::EventMsg(EventMsg::TokenCount(TokenCountEvent {
            info: Some(TokenUsageInfo {
                total_token_usage: usage.clone(),
                last_token_usage: usage,
                model_context_window: Some(200_000),
            }),
            rate_limits: None,
        })),
    }
}

fn turn_context_line(timestamp: &str, model: &str) -> RolloutLine {
    RolloutLine {
        timestamp: timestamp.to_string(),
        ordinal: None,
        item: RolloutItem::TurnContext(TurnContextItem {
            turn_id: None,
            root_turn_id: None,
            cwd: PathBuf::from("/tmp/project")
                .try_into()
                .expect("test cwd should be absolute"),
            workspace_roots: None,
            current_date: None,
            timezone: None,
            approval_policy: AskForApproval::Never,
            approvals_reviewer: None,
            sandbox_policy: SandboxPolicy::new_read_only_policy(),
            permission_profile: None,
            active_permission_profile: None,
            network: None,
            file_system_sandbox_policy: None,
            model: model.to_string(),
            comp_hash: None,
            personality: None,
            collaboration_mode: None,
            multi_agent_version: None,
            multi_agent_mode: None,
            realtime_active: None,
            cyber_access_program: None,
            effort: None,
            summary: ReasoningSummary::Auto,
        }),
    }
}

fn user_prompt_line(timestamp: &str, text: &str) -> RolloutLine {
    RolloutLine {
        timestamp: timestamp.to_string(),
        ordinal: None,
        item: RolloutItem::ResponseItem(ResponseItemEnvelope::new(ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        })),
    }
}

fn assert_cost_close(actual: f64, expected: f64) {
    let delta = (actual - expected).abs();
    assert!(
        delta < 0.00000001,
        "expected {actual} to be within epsilon of {expected}"
    );
}
