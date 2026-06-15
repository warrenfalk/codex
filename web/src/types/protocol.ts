export type { ClientInfo } from "../../../codex-rs/app-server-protocol/schema/typescript/ClientInfo";
export type { ClientRequest } from "../../../codex-rs/app-server-protocol/schema/typescript/ClientRequest";
export type { InitializeCapabilities } from "../../../codex-rs/app-server-protocol/schema/typescript/InitializeCapabilities";
export type { InitializeResponse } from "../../../codex-rs/app-server-protocol/schema/typescript/InitializeResponse";
export type { RequestId } from "../../../codex-rs/app-server-protocol/schema/typescript/RequestId";
export type { ServerNotification } from "../../../codex-rs/app-server-protocol/schema/typescript/ServerNotification";
export type { ServerRequest } from "../../../codex-rs/app-server-protocol/schema/typescript/ServerRequest";
export type { JsonValue } from "../../../codex-rs/app-server-protocol/schema/typescript/serde_json/JsonValue";
export type {
  AgentMessageDeltaNotification,
  ChatgptAuthTokensRefreshParams,
  ChatgptAuthTokensRefreshResponse,
  CommandExecutionApprovalDecision,
  CommandExecutionOutputDeltaNotification,
  CommandExecutionRequestApprovalParams,
  CommandExecutionRequestApprovalResponse,
  DynamicToolCallOutputContentItem,
  DynamicToolCallParams,
  DynamicToolCallResponse,
  ErrorNotification,
  FileChangeOutputDeltaNotification,
  FileChangeRequestApprovalParams,
  FileChangeRequestApprovalResponse,
  FileChangeApprovalDecision,
  GrantedPermissionProfile,
  ItemCompletedNotification,
  ItemStartedNotification,
  McpServerElicitationRequestParams,
  McpServerElicitationRequestResponse,
  PermissionGrantScope,
  PermissionsRequestApprovalParams,
  PermissionsRequestApprovalResponse,
  PlanDeltaNotification,
  ReasoningSummaryPartAddedNotification,
  ReasoningSummaryTextDeltaNotification,
  ReasoningTextDeltaNotification,
  ServerRequestResolvedNotification,
  Thread,
  ThreadArchiveResponse,
  ThreadArchivedNotification,
  ThreadClosedNotification,
  ThreadItem,
  ThreadListResponse,
  ThreadSetNameResponse,
  ThreadNameUpdatedNotification,
  ThreadResumeParams,
  ThreadResumeResponse,
  ThreadStartedNotification,
  ThreadStatus,
  ThreadStatusChangedNotification,
  ThreadUnarchivedNotification,
  TurnItemsView,
  ToolRequestUserInputParams,
  ToolRequestUserInputResponse,
  Turn,
  TurnCompletedNotification,
  TurnInterruptResponse,
  TurnStartResponse,
  TurnStartedNotification,
  TurnStatus,
  UserInput,
} from "../../../codex-rs/app-server-protocol/schema/typescript/v2";

export const knownServerRequestMethods = [
  "item/commandExecution/requestApproval",
  "item/fileChange/requestApproval",
  "item/tool/requestUserInput",
  "mcpServer/elicitation/request",
  "item/permissions/requestApproval",
  "item/tool/call",
  "account/chatgptAuthTokens/refresh",
  "applyPatchApproval",
  "execCommandApproval",
] as const;

export type KnownServerRequestMethod =
  (typeof knownServerRequestMethods)[number];

export type UnknownServerRequest = {
  method: string;
  id: string | number;
  params: unknown;
  unknown: true;
};

export type ExperimentalThreadResumeParams =
  import("../../../codex-rs/app-server-protocol/schema/typescript/v2/ThreadResumeParams").ThreadResumeParams & {
    excludeTurns?: boolean;
    initialTurnsPage?: {
      itemsView?:
        | import("../../../codex-rs/app-server-protocol/schema/typescript/v2/TurnItemsView").TurnItemsView
        | null;
      limit?: number | null;
      sortDirection?: "asc" | "desc" | null;
    } | null;
  };

export type ExperimentalThreadResumeResponse =
  import("../../../codex-rs/app-server-protocol/schema/typescript/v2/ThreadResumeResponse").ThreadResumeResponse & {
    initialTurnsPage?: ThreadTurnsListResponse | null;
  };

export type ExperimentalThreadTurnsListParams = {
  cursor?: string | null;
  itemsView?:
    | import("../../../codex-rs/app-server-protocol/schema/typescript/v2/TurnItemsView").TurnItemsView
    | null;
  limit?: number | null;
  sortDirection?: "asc" | "desc" | null;
  threadId: string;
};

export type ThreadTurnsListResponse = {
  backwardsCursor: string | null;
  data: import("../../../codex-rs/app-server-protocol/schema/typescript/v2/Turn").Turn[];
  nextCursor: string | null;
};

export type AnyServerRequest =
  | import("../../../codex-rs/app-server-protocol/schema/typescript/ServerRequest").ServerRequest
  | UnknownServerRequest;

export function isKnownServerRequestMethod(
  method: string,
): method is KnownServerRequestMethod {
  return knownServerRequestMethods.includes(method as KnownServerRequestMethod);
}
