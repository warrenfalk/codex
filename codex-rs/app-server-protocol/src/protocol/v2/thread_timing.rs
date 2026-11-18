//! Read-only turn telemetry. Request and span identities match durable timing records.

use crate::JsonSchema;
use crate::TS;
use codex_history::timing as history;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingReadParams {
    pub thread_id: String,
    #[ts(optional = nullable)]
    pub turn_id: Option<String>,
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingReadResponse {
    pub data: Vec<ThreadTimingEntry>,
    /// Process-local provisional counters, replaced by durable completions when available.
    pub active: Vec<ThreadTimingActiveSpan>,
    /// Resume after the scanned prefix, including when data is empty or at EOF.
    pub cursor: String,
    /// Continue backfill immediately when present; otherwise wait for new observations.
    pub next_cursor: Option<String>,
    #[ts(type = "number")]
    pub observed_at_ms: i64,
    /// Malformed rollout lines or unsupported timing records encountered in this page.
    pub skipped_record_count: u32,
}

/// A notification page is separate from the RPC response in exported schema namespaces.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingPage {
    pub data: Vec<ThreadTimingEntry>,
    pub active: Vec<ThreadTimingActiveSpan>,
    pub cursor: String,
    pub next_cursor: Option<String>,
    #[ts(type = "number")]
    pub observed_at_ms: i64,
    pub skipped_record_count: u32,
}

impl From<ThreadTimingReadResponse> for ThreadTimingPage {
    fn from(page: ThreadTimingReadResponse) -> Self {
        Self {
            data: page.data,
            active: page.active,
            cursor: page.cursor,
            next_cursor: page.next_cursor,
            observed_at_ms: page.observed_at_ms,
            skipped_record_count: page.skipped_record_count,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingActiveSpan {
    pub record: ThreadTimingRecord,
    #[ts(type = "number")]
    pub elapsed_us: u64,
    pub stream: Option<ThreadTimingStream>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingSubscribeParams {
    /// Client-chosen connection-local identity, also present on early notifications.
    pub subscription_id: String,
    pub thread_id: String,
    #[ts(optional = nullable)]
    pub turn_id: Option<String>,
    /// Omit to backfill from the beginning, or pass a read/update cursor to resume.
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingSubscribeResponse {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingUnsubscribeParams {
    pub subscription_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingUnsubscribeResponse {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingUpdatedNotification {
    pub subscription_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub page: ThreadTimingPage,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingSubscriptionClosedNotification {
    pub subscription_id: String,
    pub thread_id: String,
    /// A failed reader requires an explicit resubscription; its last cursor stays valid.
    pub error: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingEntry {
    /// Stable identity in this rollout, suitable for deduplicating a replayed page.
    pub id: String,
    pub record: ThreadTimingRecord,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingRecord {
    pub version: u8,
    pub thread_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub attempt: u32,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub phase: ThreadTimingPhase,
    pub event: ThreadTimingEvent,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum ThreadTimingPhase {
    PrepareRequest,
    SamplingAttempt,
    ModelRequest,
    TransportAttempt,
    StreamSetup,
    ModelCatalogRefresh,
    ToolWait,
    RetryRecovery,
    ModelOutput,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum ThreadTimingStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    export_to = "v2/"
)]
pub enum ThreadTimingEvent {
    Started {
        #[ts(type = "number")]
        at_ms: i64,
        model: String,
    },
    Completed {
        #[ts(type = "number")]
        at_ms: i64,
        #[ts(type = "number")]
        duration_us: u64,
        status: ThreadTimingStatus,
        response_id: Option<String>,
        stream: Option<ThreadTimingStream>,
    },
    Item {
        #[ts(type = "number")]
        at_ms: i64,
        item_id: String,
        item_type: String,
        boundary: ThreadTimingItemBoundary,
        stream: Option<ThreadTimingItem>,
    },
    ResponseCreated {
        #[ts(type = "number")]
        at_ms: i64,
        response_id: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum ThreadTimingItemBoundary {
    Added,
    Done,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingItem {
    #[ts(type = "number")]
    pub delta_count: u64,
    #[ts(type = "number")]
    pub delta_bytes: u64,
    #[ts(type = "number | null")]
    pub first_delta_at_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub last_delta_at_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadTimingStream {
    #[ts(type = "number")]
    pub event_count: u64,
    #[ts(type = "number")]
    pub delta_count: u64,
    #[ts(type = "number")]
    pub delta_bytes: u64,
    #[ts(type = "number | null")]
    pub first_event_at_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub last_event_at_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub first_delta_at_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub last_delta_at_ms: Option<i64>,
    #[ts(type = "number")]
    pub receive_wait_us: u64,
    #[ts(type = "number")]
    pub event_processing_us: u64,
    #[ts(type = "number")]
    pub omitted_item_count: u64,
}

impl From<history::TimingRecord> for ThreadTimingRecord {
    fn from(record: history::TimingRecord) -> Self {
        use history::TimingEvent as Event;
        Self {
            version: record.version,
            thread_id: record.thread_id.to_string(),
            turn_id: record.turn_id,
            request_id: record.request_id,
            attempt: record.attempt,
            span_id: record.span_id,
            parent_span_id: record.parent_span_id,
            phase: match record.phase {
                history::TimingPhase::PrepareRequest => ThreadTimingPhase::PrepareRequest,
                history::TimingPhase::SamplingAttempt => ThreadTimingPhase::SamplingAttempt,
                history::TimingPhase::ModelRequest => ThreadTimingPhase::ModelRequest,
                history::TimingPhase::TransportAttempt => ThreadTimingPhase::TransportAttempt,
                history::TimingPhase::StreamSetup => ThreadTimingPhase::StreamSetup,
                history::TimingPhase::ModelCatalogRefresh => ThreadTimingPhase::ModelCatalogRefresh,
                history::TimingPhase::ToolWait => ThreadTimingPhase::ToolWait,
                history::TimingPhase::RetryRecovery => ThreadTimingPhase::RetryRecovery,
                history::TimingPhase::ModelOutput => ThreadTimingPhase::ModelOutput,
            },
            event: match record.event {
                Event::Started { at_ms, model } => ThreadTimingEvent::Started { at_ms, model },
                Event::Completed {
                    at_ms,
                    duration_us,
                    status,
                    response_id,
                    stream,
                } => ThreadTimingEvent::Completed {
                    at_ms,
                    duration_us,
                    response_id,
                    status: match status {
                        history::TimingStatus::Completed => ThreadTimingStatus::Completed,
                        history::TimingStatus::Failed => ThreadTimingStatus::Failed,
                        history::TimingStatus::Cancelled => ThreadTimingStatus::Cancelled,
                    },
                    stream: stream.map(Into::into),
                },
                Event::Item {
                    at_ms,
                    item_id,
                    item_type,
                    boundary,
                    stream,
                } => ThreadTimingEvent::Item {
                    at_ms,
                    item_id,
                    item_type,
                    boundary: match boundary {
                        history::ItemBoundary::Added => ThreadTimingItemBoundary::Added,
                        history::ItemBoundary::Done => ThreadTimingItemBoundary::Done,
                    },
                    stream: stream.map(|stream| ThreadTimingItem {
                        delta_count: stream.delta_count,
                        delta_bytes: stream.delta_bytes,
                        first_delta_at_ms: stream.first_delta_at_ms,
                        last_delta_at_ms: stream.last_delta_at_ms,
                    }),
                },
                Event::ResponseCreated { at_ms, response_id } => {
                    ThreadTimingEvent::ResponseCreated { at_ms, response_id }
                }
            },
        }
    }
}

impl From<history::StreamTiming> for ThreadTimingStream {
    fn from(stream: history::StreamTiming) -> Self {
        Self {
            event_count: stream.event_count,
            delta_count: stream.delta_count,
            delta_bytes: stream.delta_bytes,
            first_event_at_ms: stream.first_event_at_ms,
            last_event_at_ms: stream.last_event_at_ms,
            first_delta_at_ms: stream.first_delta_at_ms,
            last_delta_at_ms: stream.last_delta_at_ms,
            receive_wait_us: stream.receive_wait_us,
            event_processing_us: stream.event_processing_us,
            omitted_item_count: stream.omitted_item_count,
        }
    }
}
