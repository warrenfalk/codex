use super::UserInput;
use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

/// Registers a trusted MCP bridge connection as one external controller.
/// Authentication of this stable identity is the bridge's responsibility.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ControllerRegisterParams {
    pub controller_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ControllerRegisterResponse {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlReadParams {
    pub thread_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlRequestParams {
    pub thread_id: String,
    /// Caller-generated unique ID. Repeating the latest request is idempotent.
    pub request_id: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlUpdateParams {
    pub thread_id: String,
    pub action: ThreadControlAction,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum ThreadControlAction {
    Approve {
        #[serde(rename = "requestId")]
        #[ts(rename = "requestId")]
        request_id: String,
    },
    Deny {
        #[serde(rename = "requestId")]
        #[ts(rename = "requestId")]
        request_id: String,
    },
    Revoke,
    Lock,
    Unlock,
    Grant {
        #[serde(rename = "controllerId")]
        #[ts(rename = "controllerId")]
        controller_id: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlReleaseParams {
    pub thread_id: String,
    pub grant_id: String,
}

/// Restricted input: deliberately excludes sandbox, approval, and config overrides.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct ThreadControlInputParams {
    pub thread_id: String,
    pub grant_id: String,
    pub input: Vec<UserInput>,
    /// Null means start only if idle. A turn ID means steer exactly that turn.
    #[ts(optional = nullable)]
    pub expected_turn_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlInterruptParams {
    pub thread_id: String,
    pub grant_id: String,
    pub turn_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlInputResponse {
    pub turn_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlInterruptResponse {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlGrant {
    pub id: String,
    pub controller_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum ThreadControlRequestStatus {
    Pending,
    Granted,
    Denied,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlRequest {
    pub id: String,
    pub controller_id: String,
    pub reason: String,
    pub status: ThreadControlRequestStatus,
    pub requested_at: i64,
    pub auto_grant_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlState {
    pub thread_id: String,
    pub locked: bool,
    pub grant: Option<ThreadControlGrant>,
    /// Latest request, including its terminal decision, retained until replaced.
    pub request: Option<ThreadControlRequest>,
    pub interactive_attached: bool,
    pub active_turn_id: Option<String>,
    /// Also covers startup before an active turn ID has been published.
    pub turn_running: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlReadResponse {
    pub state: ThreadControlState,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlRequestResponse {
    pub state: ThreadControlState,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlUpdateResponse {
    pub state: ThreadControlState,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlReleaseResponse {
    pub state: ThreadControlState,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadControlChangedNotification {
    pub state: ThreadControlState,
}
