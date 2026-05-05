import type { JsonRpcRequestMessage } from "../src/lib/jsonrpc";
import type {
  AnyServerRequest,
  ServerNotification,
} from "../src/types/protocol";

export type BrowserPushMessage = {
  body: string;
  tag: string;
  title: string;
  url: string;
};

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
  return typeof value === "string" && value.trim() ? value : null;
}

function stringArrayParam(params: unknown, key: string): string[] | null {
  if (!isRecord(params)) {
    return null;
  }

  const value = params[key];
  if (!Array.isArray(value)) {
    return null;
  }

  const strings = value.filter(
    (part): part is string => typeof part === "string",
  );
  return strings.length === value.length ? strings : null;
}

function trimBody(text: string): string {
  const trimmed = text.replace(/\s+/g, " ").trim();
  return trimmed.length > 140 ? `${trimmed.slice(0, 137)}...` : trimmed;
}

function requestBody(request: AnyServerRequest): string {
  switch (request.method) {
    case "item/commandExecution/requestApproval": {
      const command = stringParam(request.params, "command");
      return command ? trimBody(command) : "Review a command before it runs.";
    }
    case "execCommandApproval": {
      const command = stringArrayParam(request.params, "command")?.join(" ");
      return command ? trimBody(command) : "Review a command before it runs.";
    }
    case "item/fileChange/requestApproval":
    case "applyPatchApproval":
      return stringParam(request.params, "reason") ?? "Review file changes.";
    case "item/permissions/requestApproval":
      return (
        stringParam(request.params, "reason") ?? "Review a permission request."
      );
    case "item/tool/requestUserInput":
      return "Answer a question from Codex.";
    case "mcpServer/elicitation/request":
      return "Answer an MCP server prompt.";
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
    title: requestTitle(serverRequest),
    url: threadUrl(threadId),
  };
}

export function pushMessageForServerNotification(
  notification: ServerNotification,
): BrowserPushMessage | null {
  switch (notification.method) {
    case "turn/completed": {
      const { threadId, turn } = notification.params;
      if (turn.status === "completed") {
        return {
          body: "Thread is ready.",
          tag: `codex-turn-${threadId}-${turn.id}-completed`,
          title: "Codex finished",
          url: threadUrl(threadId),
        };
      }

      if (turn.status === "failed") {
        return {
          body: turn.error?.message ?? "The turn failed.",
          tag: `codex-turn-${threadId}-${turn.id}-failed`,
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
