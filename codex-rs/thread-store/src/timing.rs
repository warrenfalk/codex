use codex_app_server_protocol::ThreadTimingEntry;
use codex_protocol::ThreadId;

/// Bounded, forward-only timing reads, independent of interactive attachment.
#[derive(Clone, Debug)]
pub struct ReadTimingParams {
    pub thread_id: ThreadId,
    pub turn_id: Option<String>,
    pub cursor: Option<String>,
    pub page_size: usize,
}

#[derive(Debug)]
pub struct TimingPage {
    pub data: Vec<ThreadTimingEntry>,
    pub cursor: String,
    pub next_cursor: Option<String>,
    pub skipped_record_count: u32,
}
