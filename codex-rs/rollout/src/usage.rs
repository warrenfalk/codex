use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::RolloutLine;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::TokenUsage;
use serde::Serialize;
use serde_json::Value;
use time::Date;
use time::Duration;
use time::OffsetDateTime;
use time::PrimitiveDateTime;
use time::Time;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use tokio::io::AsyncBufReadExt;
use uuid::Uuid;

use crate::ARCHIVED_SESSIONS_SUBDIR;
use crate::SESSIONS_SUBDIR;
use crate::session_index;
use crate::usage_pricing::UsageCostBreakdown;
use crate::usage_pricing::estimate_openai_standard_cost;
use crate::usage_pricing::pricing_lookup_description;

#[derive(Debug, Clone, Copy)]
pub struct UsageReportPeriod {
    pub since: OffsetDateTime,
    pub until: OffsetDateTime,
}

impl UsageReportPeriod {
    pub fn from_args(
        last: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        now: OffsetDateTime,
    ) -> anyhow::Result<Self> {
        let until = match until {
            Some(until) => parse_time_boundary(until, TimeBoundary::Until)?,
            None => now,
        };
        let since = match (last, since) {
            (Some(_), Some(_)) => {
                anyhow::bail!("--last cannot be combined with --since");
            }
            (Some(last), None) => until - parse_duration(last)?,
            (None, Some(since)) => parse_time_boundary(since, TimeBoundary::Since)?,
            (None, None) => until - Duration::days(7),
        };

        if since >= until {
            anyhow::bail!("usage report start must be before end");
        }

        Ok(Self { since, until })
    }
}

#[derive(Debug, Clone, Copy)]
enum TimeBoundary {
    Since,
    Until,
}

#[derive(Debug, Clone)]
pub struct UsageReportOptions {
    pub since: OffsetDateTime,
    pub until: OffsetDateTime,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub since: String,
    pub until: String,
    pub generated_at: String,
    pub totals: UsageBreakdown,
    pub costs: UsageCostBreakdown,
    pub threads: Vec<ThreadUsageReport>,
    pub scanned_files: usize,
    pub matched_files: usize,
    pub parse_errors: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBreakdown {
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub uncached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
}

impl UsageBreakdown {
    fn add_usage(&mut self, usage: &TokenUsage) {
        self.total_tokens += usage.total_tokens.max(0);
        self.input_tokens += usage.input_tokens.max(0);
        self.cached_input_tokens += usage.cached_input();
        self.uncached_input_tokens += usage.non_cached_input();
        self.output_tokens += usage.output_tokens.max(0);
        self.reasoning_output_tokens += usage.reasoning_output_tokens.max(0);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsageReport {
    pub thread_id: Option<String>,
    pub thread_name: Option<String>,
    pub first_prompt: Option<String>,
    pub display_name: String,
    pub forked_from_id: Option<String>,
    pub model_provider: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub first_usage_at: String,
    pub last_usage_at: String,
    pub usage_events: usize,
    pub usage: UsageBreakdown,
    pub costs: UsageCostBreakdown,
    pub rollout_path: PathBuf,
}

#[derive(Debug)]
struct RolloutFile {
    path: PathBuf,
    modified_at: Option<OffsetDateTime>,
}

#[derive(Debug)]
struct UsageLine {
    timestamp: Option<OffsetDateTime>,
    item: RolloutItem,
}

#[derive(Debug, Clone)]
struct ReportMetadata {
    thread_id: Option<ThreadId>,
    forked_from_id: Option<ThreadId>,
    model_provider: Option<String>,
    model: Option<String>,
    cwd: Option<PathBuf>,
    first_prompt: Option<String>,
}

#[derive(Debug)]
struct ThreadUsageAccumulator {
    metadata: ReportMetadata,
    usage: UsageBreakdown,
    costs: UsageCostBreakdown,
    first_usage_at: OffsetDateTime,
    last_usage_at: OffsetDateTime,
    usage_events: usize,
    rollout_path: PathBuf,
}

pub async fn generate_usage_report(
    codex_home: &Path,
    options: UsageReportOptions,
) -> anyhow::Result<UsageReport> {
    let roots = [
        codex_home.join(SESSIONS_SUBDIR),
        codex_home.join(ARCHIVED_SESSIONS_SUBDIR),
    ];
    let mut files = Vec::new();
    for root in roots {
        collect_rollout_files(&root, &mut files).await?;
    }

    let scanned_files = files.len();
    let path_by_thread_id = index_paths_by_thread_id(&files);
    let candidates = files
        .into_iter()
        .filter(|file| {
            file.modified_at
                .is_none_or(|modified_at| modified_at >= options.since)
        })
        .collect::<Vec<_>>();
    let matched_files = candidates.len();

    let mut parent_cache: HashMap<PathBuf, Vec<Value>> = HashMap::new();
    let mut parse_errors = 0usize;
    let mut warnings = Vec::new();
    let mut missing_pricing = HashSet::new();
    let mut threads = Vec::new();

    for file in candidates {
        let lines = read_usage_lines(&file.path).await?;
        parse_errors += lines.parse_errors;
        if let Some(report) = usage_for_rollout(
            &file.path,
            &lines.lines,
            &options,
            &path_by_thread_id,
            &mut parent_cache,
            &mut warnings,
            &mut missing_pricing,
        )
        .await?
        {
            threads.push(report);
        }
    }

    threads.sort_by(|a, b| {
        b.costs
            .total_usd
            .total_cmp(&a.costs.total_usd)
            .then_with(|| b.usage.total_tokens.cmp(&a.usage.total_tokens))
            .then_with(|| b.last_usage_at.cmp(&a.last_usage_at))
            .then_with(|| a.rollout_path.cmp(&b.rollout_path))
    });

    let mut totals = UsageBreakdown::default();
    let mut costs = UsageCostBreakdown::default();
    for thread in &threads {
        totals.total_tokens += thread.usage.total_tokens;
        totals.input_tokens += thread.usage.input_tokens;
        totals.cached_input_tokens += thread.usage.cached_input_tokens;
        totals.uncached_input_tokens += thread.usage.uncached_input_tokens;
        totals.output_tokens += thread.usage.output_tokens;
        totals.reasoning_output_tokens += thread.usage.reasoning_output_tokens;
        costs.add_assign(&thread.costs);
    }
    let thread_names = thread_names_for_report(codex_home, &threads).await?;
    let thread_reports = threads
        .into_iter()
        .map(|thread| {
            let thread_name = thread
                .metadata
                .thread_id
                .as_ref()
                .and_then(|thread_id| thread_names.get(thread_id))
                .cloned();
            thread.finish(thread_name)
        })
        .collect();

    Ok(UsageReport {
        since: format_time(options.since),
        until: format_time(options.until),
        generated_at: format_time(OffsetDateTime::now_utc()),
        totals,
        costs,
        threads: thread_reports,
        scanned_files,
        matched_files,
        parse_errors,
        warnings,
    })
}

struct UsageLines {
    lines: Vec<UsageLine>,
    parse_errors: usize,
}

async fn usage_for_rollout(
    path: &Path,
    lines: &[UsageLine],
    options: &UsageReportOptions,
    path_by_thread_id: &HashMap<ThreadId, PathBuf>,
    parent_cache: &mut HashMap<PathBuf, Vec<Value>>,
    warnings: &mut Vec<String>,
    missing_pricing: &mut HashSet<String>,
) -> anyhow::Result<Option<ThreadUsageAccumulator>> {
    let Some(mut metadata) = metadata_from_lines(lines) else {
        return Ok(None);
    };
    let copied_prefix_end = copied_prefix_end(
        path,
        lines,
        metadata.forked_from_id,
        path_by_thread_id,
        parent_cache,
        warnings,
    )
    .await?;
    metadata.first_prompt = first_prompt_text(lines, copied_prefix_end);

    let mut active_metadata = metadata.clone();
    let mut previous_usage: Option<TokenUsage> = None;
    let mut accumulator: Option<ThreadUsageAccumulator> = None;

    for (index, line) in lines.iter().enumerate() {
        if let RolloutItem::TurnContext(turn_context) = &line.item {
            active_metadata.model = Some(turn_context.model.clone());
            active_metadata.cwd = Some(turn_context.cwd.clone().to_path_buf());
        }

        let RolloutItem::EventMsg(EventMsg::TokenCount(token_count)) = &line.item else {
            continue;
        };
        let Some(info) = token_count.info.as_ref() else {
            continue;
        };
        let current_usage = info.total_token_usage.clone();

        if index < copied_prefix_end {
            previous_usage = Some(current_usage);
            continue;
        }

        let Some(timestamp) = line.timestamp else {
            previous_usage = Some(current_usage);
            continue;
        };
        if timestamp < options.since {
            previous_usage = Some(current_usage);
            continue;
        }
        if timestamp >= options.until {
            previous_usage = Some(current_usage);
            continue;
        }

        let usage_delta = usage_delta(previous_usage.as_ref(), &current_usage);
        previous_usage = Some(current_usage);
        if usage_delta.total_tokens <= 0 {
            continue;
        }

        let entry = accumulator.get_or_insert_with(|| ThreadUsageAccumulator {
            metadata: active_metadata.clone(),
            usage: UsageBreakdown::default(),
            costs: UsageCostBreakdown::default(),
            first_usage_at: timestamp,
            last_usage_at: timestamp,
            usage_events: 0,
            rollout_path: path.to_path_buf(),
        });
        entry.metadata = active_metadata.clone();
        entry.usage.add_usage(&usage_delta);
        match estimate_openai_standard_cost(
            active_metadata.model_provider.as_deref(),
            active_metadata.model.as_deref(),
            &usage_delta,
        ) {
            Ok(costs) => entry.costs.add_assign(&costs),
            Err(_) => {
                let pricing_description = pricing_lookup_description(
                    active_metadata.model_provider.as_deref(),
                    active_metadata.model.as_deref(),
                );
                if missing_pricing.insert(pricing_description.clone()) {
                    warnings.push(format!(
                        "no OpenAI API pricing configured for {pricing_description}; cost estimate excludes those tokens"
                    ));
                }
            }
        }
        entry.first_usage_at = entry.first_usage_at.min(timestamp);
        entry.last_usage_at = entry.last_usage_at.max(timestamp);
        entry.usage_events += 1;
    }

    Ok(accumulator)
}

async fn copied_prefix_end(
    path: &Path,
    lines: &[UsageLine],
    forked_from_id: Option<ThreadId>,
    path_by_thread_id: &HashMap<ThreadId, PathBuf>,
    parent_cache: &mut HashMap<PathBuf, Vec<Value>>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<usize> {
    let Some(forked_from_id) = forked_from_id else {
        return Ok(0);
    };
    let Some(parent_path) = path_by_thread_id.get(&forked_from_id) else {
        let fallback_end = copied_prefix_end_by_fork_timestamp(lines);
        warnings.push(format!(
            "unable to resolve fork parent {forked_from_id} for {}; skipped {} fork-time copied lines by timestamp only",
            path.display(),
            fallback_end.saturating_sub(1),
        ));
        return Ok(fallback_end);
    };
    let parent_values = load_parent_item_values(parent_path, parent_cache).await?;
    let mut copied_prefix_end = 1usize;
    for (offset, parent_value) in parent_values.iter().enumerate() {
        let child_index = offset + 1;
        let Some(child_line) = lines.get(child_index) else {
            break;
        };
        let child_value = serde_json::to_value(&child_line.item)?;
        if child_value != *parent_value {
            break;
        }
        copied_prefix_end = child_index + 1;
    }
    Ok(copied_prefix_end)
}

fn copied_prefix_end_by_fork_timestamp(lines: &[UsageLine]) -> usize {
    let Some(fork_timestamp) = lines.first().and_then(|line| line.timestamp) else {
        return 1;
    };

    lines
        .iter()
        .skip(1)
        .take_while(|line| line.timestamp == Some(fork_timestamp))
        .count()
        + 1
}

async fn load_parent_item_values(
    parent_path: &Path,
    parent_cache: &mut HashMap<PathBuf, Vec<Value>>,
) -> anyhow::Result<Vec<Value>> {
    if let Some(values) = parent_cache.get(parent_path) {
        return Ok(values.clone());
    }
    let lines = read_usage_lines(parent_path).await?;
    let values = lines
        .lines
        .into_iter()
        .map(|line| serde_json::to_value(line.item))
        .collect::<Result<Vec<_>, _>>()?;
    parent_cache.insert(parent_path.to_path_buf(), values.clone());
    Ok(values)
}

fn metadata_from_lines(lines: &[UsageLine]) -> Option<ReportMetadata> {
    let meta = lines.iter().find_map(|line| match &line.item {
        RolloutItem::SessionMeta(meta) => Some(meta),
        _ => None,
    })?;
    Some(metadata_from_session_meta(meta))
}

fn metadata_from_session_meta(meta: &SessionMetaLine) -> ReportMetadata {
    ReportMetadata {
        thread_id: Some(meta.meta.id),
        forked_from_id: meta.meta.forked_from_id,
        model_provider: meta.meta.model_provider.clone(),
        model: None,
        cwd: Some(meta.meta.cwd.clone()),
        first_prompt: None,
    }
}

impl ThreadUsageAccumulator {
    fn finish(self, thread_name: Option<String>) -> ThreadUsageReport {
        let thread_id = self.metadata.thread_id.map(|id| id.to_string());
        let display_name = thread_name
            .clone()
            .or_else(|| self.metadata.first_prompt.clone())
            .or_else(|| thread_id.clone())
            .unwrap_or_else(|| "-".to_string());
        ThreadUsageReport {
            thread_id,
            thread_name,
            first_prompt: self.metadata.first_prompt,
            display_name,
            forked_from_id: self.metadata.forked_from_id.map(|id| id.to_string()),
            model_provider: self.metadata.model_provider,
            model: self.metadata.model,
            cwd: self.metadata.cwd,
            first_usage_at: format_time(self.first_usage_at),
            last_usage_at: format_time(self.last_usage_at),
            usage_events: self.usage_events,
            usage: self.usage,
            costs: self.costs,
            rollout_path: self.rollout_path,
        }
    }
}

async fn thread_names_for_report(
    codex_home: &Path,
    threads: &[ThreadUsageAccumulator],
) -> anyhow::Result<HashMap<ThreadId, String>> {
    let ids = threads
        .iter()
        .filter_map(|thread| thread.metadata.thread_id)
        .collect::<HashSet<_>>();
    session_index::find_thread_names_by_ids(codex_home, &ids)
        .await
        .map_err(anyhow::Error::from)
}

fn first_prompt_text(lines: &[UsageLine], copied_prefix_end: usize) -> Option<String> {
    first_prompt_text_from_lines(&lines[copied_prefix_end.min(lines.len())..])
        .or_else(|| first_prompt_text_from_lines(lines))
}

fn first_prompt_text_from_lines(lines: &[UsageLine]) -> Option<String> {
    lines.iter().find_map(|line| {
        let RolloutItem::ResponseItem(response_item) = &line.item else {
            return None;
        };
        first_prompt_text_from_response_item(response_item)
    })
}

fn first_prompt_text_from_response_item(response_item: &ResponseItem) -> Option<String> {
    let ResponseItem::Message { role, content, .. } = response_item else {
        return None;
    };
    if role != "user" {
        return None;
    }
    content.iter().find_map(|item| {
        let ContentItem::InputText { text } = item else {
            return None;
        };
        normalize_display_text(text)
    })
}

fn normalize_display_text(text: &str) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(truncate_chars(&normalized, 120))
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn usage_delta(previous: Option<&TokenUsage>, current: &TokenUsage) -> TokenUsage {
    let Some(previous) = previous else {
        return current.clone();
    };
    TokenUsage {
        input_tokens: (current.input_tokens - previous.input_tokens).max(0),
        cached_input_tokens: (current.cached_input_tokens - previous.cached_input_tokens).max(0),
        output_tokens: (current.output_tokens - previous.output_tokens).max(0),
        reasoning_output_tokens: (current.reasoning_output_tokens
            - previous.reasoning_output_tokens)
            .max(0),
        total_tokens: (current.total_tokens - previous.total_tokens).max(0),
    }
}

async fn read_usage_lines(path: &Path) -> anyhow::Result<UsageLines> {
    let file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("failed to open rollout {}", path.display()))?;
    let reader = tokio::io::BufReader::new(file);
    let mut lines = reader.lines();
    let mut usage_lines = Vec::new();
    let mut parse_errors = 0usize;

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<RolloutLine>(trimmed) {
            Ok(line) => usage_lines.push(UsageLine {
                timestamp: parse_rollout_timestamp(line.timestamp.as_str()),
                item: line.item,
            }),
            Err(_) => {
                parse_errors += 1;
            }
        }
    }

    Ok(UsageLines {
        lines: usage_lines,
        parse_errors,
    })
}

async fn collect_rollout_files(root: &Path, files: &mut Vec<RolloutFile>) -> anyhow::Result<()> {
    if !tokio::fs::try_exists(root).await.unwrap_or(false) {
        return Ok(());
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .with_context(|| format!("failed to read rollout directory {}", dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            let path = entry.path();
            if metadata.is_dir() {
                stack.push(path);
            } else if is_rollout_file(&path) {
                let modified_at = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| truncate_to_millis(OffsetDateTime::from(modified)));
                files.push(RolloutFile { path, modified_at });
            }
        }
    }
    Ok(())
}

fn index_paths_by_thread_id(files: &[RolloutFile]) -> HashMap<ThreadId, PathBuf> {
    let mut indexed: HashMap<ThreadId, &RolloutFile> = HashMap::new();
    for file in files {
        let Some(thread_id) = thread_id_from_rollout_path(&file.path) else {
            continue;
        };
        let should_replace = indexed
            .get(&thread_id)
            .is_none_or(|existing| file.modified_at > existing.modified_at);
        if should_replace {
            indexed.insert(thread_id, file);
        }
    }
    indexed
        .into_iter()
        .map(|(thread_id, file)| (thread_id, file.path.clone()))
        .collect()
}

fn is_rollout_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
}

fn thread_id_from_rollout_path(path: &Path) -> Option<ThreadId> {
    let name = path.file_name()?.to_str()?;
    let name = name.strip_suffix(".jsonl")?;
    let uuid = name.rsplit('-').take(5).collect::<Vec<_>>();
    if uuid.len() != 5 {
        return None;
    }
    let uuid = uuid.into_iter().rev().collect::<Vec<_>>().join("-");
    Uuid::parse_str(&uuid).ok()?;
    ThreadId::from_string(&uuid).ok()
}

fn parse_time_boundary(raw: &str, boundary: TimeBoundary) -> anyhow::Result<OffsetDateTime> {
    if let Ok(timestamp) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Ok(timestamp);
    }

    let date_format = format_description!("[year]-[month]-[day]");
    if let Ok(date) = Date::parse(raw, date_format) {
        let date = match boundary {
            TimeBoundary::Since => date,
            TimeBoundary::Until => date
                .next_day()
                .ok_or_else(|| anyhow::anyhow!("date is out of range"))?,
        };
        return Ok(date.with_time(Time::MIDNIGHT).assume_utc());
    }

    anyhow::bail!("invalid timestamp `{raw}`; use RFC3339 or YYYY-MM-DD");
}

fn parse_duration(raw: &str) -> anyhow::Result<Duration> {
    let Some(unit) = raw.chars().last() else {
        anyhow::bail!("duration cannot be empty");
    };
    let count = raw[..raw.len() - unit.len_utf8()]
        .parse::<i64>()
        .with_context(|| format!("invalid duration `{raw}`"))?;
    if count <= 0 {
        anyhow::bail!("duration must be positive");
    }
    match unit {
        'h' => Ok(Duration::hours(count)),
        'd' => Ok(Duration::days(count)),
        'w' => Ok(Duration::weeks(count)),
        _ => anyhow::bail!("invalid duration `{raw}`; use h, d, or w"),
    }
}

fn parse_rollout_timestamp(raw: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(raw, &Rfc3339).ok().or_else(|| {
        let format = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
        PrimitiveDateTime::parse(raw, format)
            .ok()
            .map(PrimitiveDateTime::assume_utc)
    })
}

fn format_time(timestamp: OffsetDateTime) -> String {
    timestamp
        .format(&Rfc3339)
        .unwrap_or_else(|_| timestamp.unix_timestamp().to_string())
}

fn truncate_to_millis(dt: OffsetDateTime) -> Option<OffsetDateTime> {
    let millis_nanos = (dt.nanosecond() / 1_000_000) * 1_000_000;
    dt.replace_nanosecond(millis_nanos).ok()
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;

    use codex_protocol::config_types::ReasoningSummary;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::protocol::RolloutItem;
    use codex_protocol::protocol::RolloutLine;
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
        let child_id =
            ThreadId::from_string("00000000-0000-0000-0000-000000000020").expect("valid id");
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
        let child_id =
            ThreadId::from_string("00000000-0000-0000-0000-000000000040").expect("valid id");
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
            item: RolloutItem::SessionMeta(SessionMetaLine {
                meta: SessionMeta {
                    session_id: thread_id.into(),
                    id: thread_id,
                    forked_from_id,
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
                    memory_mode: None,
                    multi_agent_version: None,
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
            output_tokens,
            reasoning_output_tokens,
            total_tokens,
        };
        RolloutLine {
            timestamp: timestamp.to_string(),
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
            item: RolloutItem::TurnContext(TurnContextItem {
                turn_id: None,
                cwd: PathBuf::from("/tmp/project")
                    .try_into()
                    .expect("test cwd should be absolute"),
                workspace_roots: None,
                current_date: None,
                timezone: None,
                approval_policy: AskForApproval::Never,
                sandbox_policy: SandboxPolicy::new_read_only_policy(),
                permission_profile: None,
                network: None,
                file_system_sandbox_policy: None,
                model: model.to_string(),
                comp_hash: None,
                personality: None,
                collaboration_mode: None,
                multi_agent_version: None,
                multi_agent_mode: None,
                realtime_active: None,
                effort: None,
                summary: ReasoningSummary::Auto,
            }),
        }
    }

    fn user_prompt_line(timestamp: &str, text: &str) -> RolloutLine {
        RolloutLine {
            timestamp: timestamp.to_string(),
            item: RolloutItem::ResponseItem(ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: text.to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }),
        }
    }

    fn assert_cost_close(actual: f64, expected: f64) {
        let delta = (actual - expected).abs();
        assert!(
            delta < 0.00000001,
            "expected {actual} to be within epsilon of {expected}"
        );
    }
}
