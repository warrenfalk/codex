//! Model-invisible timing observations retained with a session rollout.

use codex_protocol::ThreadId;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// A client observation, never an assertion about server-side reasoning compute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimingRecord {
    pub version: u8,
    pub thread_id: ThreadId,
    pub turn_id: String,
    /// One logical sampling request, shared across stream-recovery attempts.
    pub request_id: String,
    pub attempt: u32,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub phase: TimingPhase,
    #[serde(flatten)]
    pub event: TimingEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimingPhase {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimingStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TimingEvent {
    Started {
        at_ms: i64,
        model: String,
    },
    Completed {
        at_ms: i64,
        /// Monotonic elapsed time; wall-clock adjustments do not change this value.
        duration_us: u64,
        status: TimingStatus,
        response_id: Option<String>,
        stream: Option<StreamTiming>,
    },
    Item {
        at_ms: i64,
        item_id: String,
        item_type: String,
        boundary: ItemBoundary,
        stream: Option<ItemTiming>,
    },
    ResponseCreated {
        at_ms: i64,
        response_id: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemBoundary {
    Added,
    Done,
}

/// Only deltas attributable to this item; request waits are measured separately.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ItemTiming {
    pub delta_count: u64,
    pub delta_bytes: u64,
    pub first_delta_at_ms: Option<i64>,
    pub last_delta_at_ms: Option<i64>,
}

/// Aggregates deltas without retaining their contents or writing a record per token.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StreamTiming {
    pub event_count: u64,
    pub delta_count: u64,
    pub delta_bytes: u64,
    pub first_event_at_ms: Option<i64>,
    pub last_event_at_ms: Option<i64>,
    pub first_delta_at_ms: Option<i64>,
    pub last_delta_at_ms: Option<i64>,
    /// Time the harness awaited its next stream event, including channel buffering effects.
    pub receive_wait_us: u64,
    /// Time handling received events, including awaited metadata/persistence operations.
    pub event_processing_us: u64,
    /// Added items omitted for missing identity or the bounded concurrent-item limit.
    pub omitted_item_count: u64,
}
