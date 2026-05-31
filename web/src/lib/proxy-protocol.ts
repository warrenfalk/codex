import type { Thread } from "../types/protocol";
import type { ThreadListResponse } from "../types/protocol";

export const PROXY_THREAD_LIST_UPDATED_METHOD =
  "codex-web/threadListUpdated" as const;
export const PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD =
  "codex-web/pushSubscriptionUpdated" as const;

export type ProxyThreadActivityState = "virgin" | "working" | "ready";

export type ProxyThreadActivity = {
  lastAgentMessage: string | null;
  lastUserMessage: string | null;
  state: ProxyThreadActivityState;
};

export type ProxyThreadListSnapshot = {
  previewsByThreadId: Record<string, string>;
  threadActivityByThreadId: Record<string, ProxyThreadActivity>;
  threads: Thread[];
};

export type ProxyThreadListUpdatedNotification = {
  method: typeof PROXY_THREAD_LIST_UPDATED_METHOD;
  params: ProxyThreadListSnapshot;
};

export type ProxyPushSubscriptionUpdatedNotification = {
  method: typeof PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD;
  params: {
    foregroundThreadId: string | null;
    pushSubscriptionEndpoint: string | null;
  };
};

export type ProxyThreadListResponse = ThreadListResponse & {
  previewsByThreadId: Record<string, string>;
  threadActivityByThreadId: Record<string, ProxyThreadActivity>;
};

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function isProxyThreadListUpdatedNotification(
  value: unknown,
): value is ProxyThreadListUpdatedNotification {
  return (
    isObject(value) &&
    value.method === PROXY_THREAD_LIST_UPDATED_METHOD &&
    "params" in value &&
    isObject(value.params)
  );
}
