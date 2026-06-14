import type { JsonRpcRequestMessage } from "../src/lib/jsonrpc";
import type { ServerNotification, Thread } from "../src/types/protocol";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function threadIdFromValue(value: unknown): string | null {
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
    const threadId = threadIdFromValue(nested);
    if (threadId) {
      return threadId;
    }
  }

  return null;
}

export function threadIdFromServerRequest(
  request: JsonRpcRequestMessage,
): string | null {
  return threadIdFromValue(request.params);
}

export function threadIdFromServerNotification(
  notification: ServerNotification,
): string | null {
  return threadIdFromValue(notification.params);
}

export function isSubagentThread(thread: Thread): boolean {
  return (
    thread.threadSource === "subagent" ||
    (isRecord(thread.source) && "subAgent" in thread.source)
  );
}

export function cachedThreadMayNotify(
  thread: Thread | null | undefined,
): boolean {
  return Boolean(thread && !isSubagentThread(thread));
}
