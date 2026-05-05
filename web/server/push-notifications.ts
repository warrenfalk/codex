import type { JsonRpcRequestMessage } from "../src/lib/jsonrpc";
import type {
  AnyServerRequest,
  ServerNotification,
  ThreadItem,
} from "../src/types/protocol";

export type BrowserPushMessage = {
  body: string;
  tag: string;
  threadId: string | null;
  title: string;
  url: string;
};

export type PushNotificationContext = {
  completedTurnAgentMessage?: string | null;
};

type AgentMessageItem = Extract<ThreadItem, { type: "agentMessage" }>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getThreadIdFromValue(value: unknown): string | null {
  if (!isRecord(value)) {
    return null;
  }

  if (typeof value.threadId === "string") {
    return value.threadId;
  }

  if (typeof value.conversationId === "string") {
    return value.conversationId;
  }

  for (const nested of Object.values(value)) {
    const threadId = getThreadIdFromValue(nested);
    if (threadId) {
      return threadId;
    }
  }

  return null;
}

function threadUrl(threadId: string | null): string {
  return threadId ? `/threads/${encodeURIComponent(threadId)}` : "/";
}

function stringParam(params: unknown, key: string): string | null {
  if (!isRecord(params)) {
    return null;
  }

  const value = params[key];
  if (typeof value !== "string") {
    return null;
  }

  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function stringArrayValue(value: unknown): string[] | null {
  if (!Array.isArray(value)) {
    return null;
  }

  const strings = value
    .map((part) => (typeof part === "string" ? part.trim() : null))
    .filter((part): part is string => Boolean(part));
  return strings.length > 0 && strings.length === value.length ? strings : null;
}

function stringArrayParam(params: unknown, key: string): string[] | null {
  if (!isRecord(params)) {
    return null;
  }

  return stringArrayValue(params[key]);
}

function trimBody(text: string): string {
  const trimmed = text.replace(/\s+/g, " ").trim();
  return trimmed.length > 140 ? `${trimmed.slice(0, 137)}...` : trimmed;
}

function isNonEmptyAgentMessage(item: ThreadItem): item is AgentMessageItem {
  return item.type === "agentMessage" && Boolean(item.text.trim());
}

function summarizeList(label: string, values: string[]): string | null {
  if (values.length === 0) {
    return null;
  }

  const shown = values.slice(0, 3).join(", ");
  const suffix = values.length > 3 ? `, +${values.length - 3} more` : "";
  return `${label}: ${shown}${suffix}`;
}

function approvalBody(
  what: string | null,
  why: string | null,
  fallback: string,
): string {
  const parts = [why, what].filter((part): part is string => Boolean(part));
  return parts.length > 0 ? trimBody(parts.join(". ")) : fallback;
}

function fileChangeSummary(params: unknown): string | null {
  if (!isRecord(params)) {
    return null;
  }

  const grantRoot = stringParam(params, "grantRoot");
  if (grantRoot) {
    return `Write root: ${grantRoot}`;
  }

  const fileChanges = params.fileChanges;
  if (isRecord(fileChanges)) {
    return summarizeList("Files", Object.keys(fileChanges));
  }

  return null;
}

function permissionsSummary(params: unknown): string | null {
  if (!isRecord(params) || !isRecord(params.permissions)) {
    return null;
  }

  const { permissions } = params;
  const parts: string[] = [];
  if (isRecord(permissions.network) && permissions.network.enabled === true) {
    parts.push("Network access");
  }

  if (isRecord(permissions.fileSystem)) {
    const read = stringArrayValue(permissions.fileSystem.read);
    const write = stringArrayValue(permissions.fileSystem.write);
    const readSummary = read ? summarizeList("Read", read) : null;
    const writeSummary = write ? summarizeList("Write", write) : null;
    if (readSummary) {
      parts.push(readSummary);
    }
    if (writeSummary) {
      parts.push(writeSummary);
    }
  }

  return parts.length > 0 ? parts.join(". ") : null;
}

function requestBody(request: AnyServerRequest): string {
  switch (request.method) {
    case "item/commandExecution/requestApproval": {
      const command = stringParam(request.params, "command");
      return approvalBody(
        command ? `Command: ${command}` : null,
        stringParam(request.params, "reason"),
        "Review a command before it runs.",
      );
    }
    case "execCommandApproval": {
      const command = stringArrayParam(request.params, "command")?.join(" ");
      return approvalBody(
        command ? `Command: ${command}` : null,
        stringParam(request.params, "reason"),
        "Review a command before it runs.",
      );
    }
    case "item/fileChange/requestApproval":
    case "applyPatchApproval":
      return approvalBody(
        fileChangeSummary(request.params),
        stringParam(request.params, "reason"),
        "Review file changes.",
      );
    case "item/permissions/requestApproval":
      return approvalBody(
        permissionsSummary(request.params),
        stringParam(request.params, "reason"),
        "Review a permission request.",
      );
    case "item/tool/requestUserInput":
      return "Answer a question from Codex.";
    case "mcpServer/elicitation/request":
      return (
        stringParam(request.params, "message") ?? "Answer an MCP server prompt."
      );
    case "item/tool/call":
      return "A browser tool call needs the web app.";
    case "account/chatgptAuthTokens/refresh":
      return "Sign in again to continue.";
    default:
      return "Codex needs a response.";
  }
}

function requestTitle(request: AnyServerRequest): string {
  switch (request.method) {
    case "item/commandExecution/requestApproval":
    case "execCommandApproval":
      return "Codex needs command approval";
    case "item/fileChange/requestApproval":
    case "applyPatchApproval":
      return "Codex needs file approval";
    case "item/permissions/requestApproval":
      return "Codex needs permission";
    case "item/tool/requestUserInput":
    case "mcpServer/elicitation/request":
      return "Codex needs your input";
    case "account/chatgptAuthTokens/refresh":
      return "Codex needs sign-in";
    default:
      return "Codex needs attention";
  }
}

export function pushMessageForServerRequest(
  request: JsonRpcRequestMessage,
): BrowserPushMessage | null {
  const serverRequest = request as AnyServerRequest;
  const threadId = getThreadIdFromValue(serverRequest.params);

  return {
    body: requestBody(serverRequest),
    tag: `codex-request-${String(serverRequest.id)}`,
    threadId,
    title: requestTitle(serverRequest),
    url: threadUrl(threadId),
  };
}

function latestCompletedAgentMessageText(
  notification: ServerNotification,
  context: PushNotificationContext,
): string | null {
  const contextText = context.completedTurnAgentMessage?.trim();
  if (contextText) {
    return contextText;
  }

  if (notification.method !== "turn/completed") {
    return null;
  }

  const agentMessages = notification.params.turn.items.filter(
    isNonEmptyAgentMessage,
  );
  const finalMessage = agentMessages
    .slice()
    .reverse()
    .find((item) => item.phase === "final_answer" || item.phase === null);
  return (finalMessage ?? agentMessages.at(-1))?.text.trim() ?? null;
}

export function pushMessageForServerNotification(
  notification: ServerNotification,
  context: PushNotificationContext = {},
): BrowserPushMessage | null {
  switch (notification.method) {
    case "turn/completed": {
      const { threadId, turn } = notification.params;
      if (turn.status === "completed") {
        return {
          body: trimBody(
            latestCompletedAgentMessageText(notification, context) ??
              "Thread is ready.",
          ),
          tag: `codex-turn-${threadId}-${turn.id}-completed`,
          threadId,
          title: "Codex finished",
          url: threadUrl(threadId),
        };
      }

      if (turn.status === "failed") {
        return {
          body: turn.error?.message ?? "The turn failed.",
          tag: `codex-turn-${threadId}-${turn.id}-failed`,
          threadId,
          title: "Codex hit an error",
          url: threadUrl(threadId),
        };
      }

      return null;
    }
    case "error": {
      const { error, threadId, turnId } = notification.params;
      return {
        body: error.message,
        tag: `codex-turn-${threadId}-${turnId}-failed`,
        threadId,
        title: notification.params.willRetry
          ? "Codex is retrying"
          : "Codex hit an error",
        url: threadUrl(threadId),
      };
    }
    default:
      return null;
  }
}
