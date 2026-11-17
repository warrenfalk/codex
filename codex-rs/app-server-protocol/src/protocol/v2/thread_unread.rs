use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadUnreadSetParams {
    pub thread_id: String,
    /// Set unread explicitly, or acknowledge it. Acknowledgement is ignored during an active turn,
    /// including a turn waiting for approval or input. Ignored attempts are never deferred.
    pub unread: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadUnreadSetResponse {
    /// Authoritative unread state after applying or ignoring the request.
    pub unread: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadUnreadChangedNotification {
    pub thread_id: String,
    pub unread: bool,
}
