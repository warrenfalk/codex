import type {
  AgentMessageDeltaNotification,
  AnyServerRequest,
  CommandExecutionOutputDeltaNotification,
  ItemCompletedNotification,
  ItemStartedNotification,
  PlanDeltaNotification,
  ReasoningSummaryPartAddedNotification,
  ReasoningSummaryTextDeltaNotification,
  ReasoningTextDeltaNotification,
  ServerNotification,
  Thread,
  ThreadItem,
  ThreadStatus,
  Turn,
  TurnCompletedNotification,
  TurnStartedNotification,
} from "@/types/protocol";

import { requestIdKey } from "./request-id";

export type ConnectionViewState =
  | "idle"
  | "connecting"
  | "connected"
  | "disconnected"
  | "error";

export type AppState = {
  connectionState: ConnectionViewState;
  connectionError: string | null;
  threads: Thread[];
  selectedThreadId: string | null;
  selectedThreadLoading: boolean;
  threadsLoading: boolean;
  pendingRequests: AnyServerRequest[];
  itemRuntimeText: Record<string, string>;
  initializeSummary: string | null;
};

export const initialState: AppState = {
  connectionState: "idle",
  connectionError: null,
  threads: [],
  selectedThreadId: null,
  selectedThreadLoading: false,
  threadsLoading: true,
  pendingRequests: [],
  itemRuntimeText: {},
  initializeSummary: null,
};

type Action =
  | { type: "connection/status"; status: ConnectionViewState; error?: string }
  | { type: "initialize/success"; summary: string }
  | { type: "threads/loading" }
  | { type: "threads/loaded"; threads: Thread[] }
  | { type: "thread/clearSelection" }
  | { type: "thread/selecting"; threadId: string }
  | { type: "thread/selected"; thread: Thread }
  | { type: "server/request"; request: AnyServerRequest }
  | { type: "server/requestResolved"; requestId: string | number }
  | { type: "server/notification"; notification: ServerNotification };

const TURN_ITEMS_VIEW_RANK: Record<Turn["itemsView"], number> = {
  notLoaded: 0,
  summary: 1,
  full: 2,
};

function sortThreads(threads: Thread[]): Thread[] {
  return [...threads].sort((left, right) => right.updatedAt - left.updatedAt);
}

function mergeThread(existing: Thread | undefined, incoming: Thread): Thread {
  if (!existing) {
    return incoming;
  }

  return {
    ...existing,
    ...incoming,
    turns: incoming.turns.length > 0 ? incoming.turns : existing.turns,
  };
}

function upsertThread(threads: Thread[], incoming: Thread): Thread[] {
  const index = threads.findIndex((thread) => thread.id === incoming.id);
  if (index === -1) {
    return sortThreads([...threads, incoming]);
  }

  const next = [...threads];
  next[index] = mergeThread(next[index], incoming);
  return sortThreads(next);
}

function updateThread(
  threads: Thread[],
  threadId: string,
  updater: (thread: Thread) => Thread,
): Thread[] {
  return threads.map((thread) =>
    thread.id === threadId ? updater(thread) : thread,
  );
}

function upsertTurn(turns: Turn[], incoming: Turn): Turn[] {
  const index = turns.findIndex((turn) => turn.id === incoming.id);
  if (index === -1) {
    return [...turns, incoming];
  }

  const next = [...turns];
  const existingTurn = next[index]!;
  const incomingItemsRank = TURN_ITEMS_VIEW_RANK[incoming.itemsView];
  const existingItemsRank = TURN_ITEMS_VIEW_RANK[existingTurn.itemsView];
  const shouldUseIncomingItems =
    incomingItemsRank > existingItemsRank ||
    (incomingItemsRank === existingItemsRank && incoming.items.length > 0);
  next[index] = {
    ...existingTurn,
    ...incoming,
    items: shouldUseIncomingItems ? incoming.items : existingTurn.items,
    itemsView: shouldUseIncomingItems
      ? incoming.itemsView
      : existingTurn.itemsView,
  };
  return next;
}

function updateTurn(
  thread: Thread,
  turnId: string,
  updater: (turn: Turn) => Turn,
): Thread {
  return {
    ...thread,
    turns: thread.turns.map((turn) =>
      turn.id === turnId ? updater(turn) : turn,
    ),
  };
}

function placeholderItem(kind: string, itemId: string): ThreadItem {
  switch (kind) {
    case "agent":
      return {
        type: "agentMessage",
        id: itemId,
        text: "",
        phase: null,
        memoryCitation: null,
      };
    case "plan":
      return { type: "plan", id: itemId, text: "" };
    case "reasoning":
      return { type: "reasoning", id: itemId, summary: [], content: [] };
    default:
      return {
        type: "agentMessage",
        id: itemId,
        text: "",
        phase: null,
        memoryCitation: null,
      };
  }
}

function upsertItem(items: ThreadItem[], incoming: ThreadItem): ThreadItem[] {
  const index = items.findIndex((item) => item.id === incoming.id);
  if (index === -1) {
    return [...items, incoming];
  }

  const next = [...items];
  next[index] = incoming;
  return next;
}

function updateItem(
  thread: Thread,
  turnId: string,
  itemId: string,
  fallbackKind: string,
  updater: (item: ThreadItem) => ThreadItem,
): Thread {
  return updateTurn(thread, turnId, (turn) => {
    const index = turn.items.findIndex((item) => item.id === itemId);
    if (index === -1) {
      return {
        ...turn,
        items: [...turn.items, updater(placeholderItem(fallbackKind, itemId))],
      };
    }

    const items = [...turn.items];
    items[index] = updater(items[index]!);
    return {
      ...turn,
      items,
    };
  });
}

function updateThreadStatus(thread: Thread, status: ThreadStatus): Thread {
  return {
    ...thread,
    status,
    updatedAt: Math.max(thread.updatedAt, Math.floor(Date.now() / 1000)),
  };
}

function applyTurnStarted(
  threads: Thread[],
  notification: TurnStartedNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) => ({
    ...thread,
    status: { type: "active", activeFlags: [] },
    updatedAt: Math.floor(Date.now() / 1000),
    turns: upsertTurn(thread.turns, notification.turn),
  }));
}

function applyTurnCompleted(
  threads: Thread[],
  notification: TurnCompletedNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) => ({
    ...thread,
    status: thread.status.type === "active" ? { type: "idle" } : thread.status,
    updatedAt: Math.floor(Date.now() / 1000),
    turns: upsertTurn(thread.turns, notification.turn),
  }));
}

function applyItemStarted(
  threads: Thread[],
  notification: ItemStartedNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateTurn(thread, notification.turnId, (turn) => ({
      ...turn,
      items: upsertItem(turn.items, notification.item),
    })),
  );
}

function applyItemCompleted(
  threads: Thread[],
  notification: ItemCompletedNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateTurn(thread, notification.turnId, (turn) => ({
      ...turn,
      items: upsertItem(turn.items, notification.item),
    })),
  );
}

function setIndexedValue(
  values: string[],
  index: number,
  nextValue: string,
): string[] {
  const next = [...values];
  while (next.length <= index) {
    next.push("");
  }
  next[index] = nextValue;
  return next;
}

function appendIndexedValue(
  values: string[],
  index: number,
  delta: string,
): string[] {
  const next = [...values];
  while (next.length <= index) {
    next.push("");
  }
  next[index] = `${next[index] ?? ""}${delta}`;
  return next;
}

function applyAgentDelta(
  threads: Thread[],
  notification: AgentMessageDeltaNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateItem(
      thread,
      notification.turnId,
      notification.itemId,
      "agent",
      (item) =>
        item.type === "agentMessage"
          ? { ...item, text: `${item.text}${notification.delta}` }
          : item,
    ),
  );
}

function applyPlanDelta(
  threads: Thread[],
  notification: PlanDeltaNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateItem(
      thread,
      notification.turnId,
      notification.itemId,
      "plan",
      (item) =>
        item.type === "plan"
          ? { ...item, text: `${item.text}${notification.delta}` }
          : item,
    ),
  );
}

function applyReasoningSummaryDelta(
  threads: Thread[],
  notification: ReasoningSummaryTextDeltaNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateItem(
      thread,
      notification.turnId,
      notification.itemId,
      "reasoning",
      (item) =>
        item.type === "reasoning"
          ? {
              ...item,
              summary: appendIndexedValue(
                item.summary,
                notification.summaryIndex,
                notification.delta,
              ),
            }
          : item,
    ),
  );
}

function applyReasoningSummaryPart(
  threads: Thread[],
  notification: ReasoningSummaryPartAddedNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateItem(
      thread,
      notification.turnId,
      notification.itemId,
      "reasoning",
      (item) =>
        item.type === "reasoning"
          ? {
              ...item,
              summary: setIndexedValue(
                item.summary,
                notification.summaryIndex,
                item.summary[notification.summaryIndex] ?? "",
              ),
            }
          : item,
    ),
  );
}

function applyReasoningTextDelta(
  threads: Thread[],
  notification: ReasoningTextDeltaNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateItem(
      thread,
      notification.turnId,
      notification.itemId,
      "reasoning",
      (item) =>
        item.type === "reasoning"
          ? {
              ...item,
              content: appendIndexedValue(
                item.content,
                notification.contentIndex,
                notification.delta,
              ),
            }
          : item,
    ),
  );
}

function applyCommandOutputDelta(
  threads: Thread[],
  notification: CommandExecutionOutputDeltaNotification,
): Thread[] {
  return updateThread(threads, notification.threadId, (thread) =>
    updateItem(
      thread,
      notification.turnId,
      notification.itemId,
      "agent",
      (item) =>
        item.type === "commandExecution"
          ? {
              ...item,
              aggregatedOutput: `${item.aggregatedOutput ?? ""}${notification.delta}`,
            }
          : item,
    ),
  );
}

function applyNotification(
  state: AppState,
  notification: ServerNotification,
): AppState {
  switch (notification.method) {
    case "thread/started":
      return {
        ...state,
        threads: upsertThread(state.threads, notification.params.thread),
      };
    case "thread/status/changed":
      return {
        ...state,
        threads: updateThread(
          state.threads,
          notification.params.threadId,
          (thread) => updateThreadStatus(thread, notification.params.status),
        ),
      };
    case "thread/archived":
      return {
        ...state,
        threads: state.threads.filter(
          (thread) => thread.id !== notification.params.threadId,
        ),
      };
    case "thread/unarchived":
      return state;
    case "thread/closed":
      return {
        ...state,
        threads: updateThread(
          state.threads,
          notification.params.threadId,
          (thread) => updateThreadStatus(thread, { type: "notLoaded" }),
        ),
      };
    case "thread/name/updated":
      return {
        ...state,
        threads: updateThread(
          state.threads,
          notification.params.threadId,
          (thread) => ({
            ...thread,
            name: notification.params.threadName ?? null,
          }),
        ),
      };
    case "turn/started":
      return {
        ...state,
        threads: applyTurnStarted(state.threads, notification.params),
      };
    case "turn/completed":
      return {
        ...state,
        threads: applyTurnCompleted(state.threads, notification.params),
      };
    case "item/started":
      return {
        ...state,
        threads: applyItemStarted(state.threads, notification.params),
      };
    case "item/completed":
      return {
        ...state,
        itemRuntimeText: {
          ...state.itemRuntimeText,
          [notification.params.item.id]: "",
        },
        threads: applyItemCompleted(state.threads, notification.params),
      };
    case "item/agentMessage/delta":
      return {
        ...state,
        threads: applyAgentDelta(state.threads, notification.params),
      };
    case "item/plan/delta":
      return {
        ...state,
        threads: applyPlanDelta(state.threads, notification.params),
      };
    case "item/reasoning/summaryTextDelta":
      return {
        ...state,
        threads: applyReasoningSummaryDelta(state.threads, notification.params),
      };
    case "item/reasoning/summaryPartAdded":
      return {
        ...state,
        threads: applyReasoningSummaryPart(state.threads, notification.params),
      };
    case "item/reasoning/textDelta":
      return {
        ...state,
        threads: applyReasoningTextDelta(state.threads, notification.params),
      };
    case "item/commandExecution/outputDelta":
      return {
        ...state,
        threads: applyCommandOutputDelta(state.threads, notification.params),
      };
    case "item/fileChange/outputDelta":
      return {
        ...state,
        itemRuntimeText: {
          ...state.itemRuntimeText,
          [notification.params.itemId]:
            `${state.itemRuntimeText[notification.params.itemId] ?? ""}${notification.params.delta}`,
        },
      };
    case "serverRequest/resolved":
      return {
        ...state,
        pendingRequests: state.pendingRequests.filter(
          (request) =>
            requestIdKey(request.id) !==
            requestIdKey(notification.params.requestId),
        ),
      };
    case "error":
      return {
        ...state,
        connectionError: notification.params.error.message,
      };
    default:
      return state;
  }
}

export function appReducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case "connection/status":
      return {
        ...state,
        connectionState: action.status,
        connectionError: action.error ?? state.connectionError,
      };
    case "initialize/success":
      return {
        ...state,
        connectionState: "connected",
        connectionError: null,
        initializeSummary: action.summary,
      };
    case "threads/loading":
      return {
        ...state,
        threadsLoading: true,
      };
    case "threads/loaded":
      return {
        ...state,
        threadsLoading: false,
        threads: sortThreads(action.threads),
      };
    case "thread/selecting":
      return {
        ...state,
        selectedThreadId: action.threadId,
        selectedThreadLoading: true,
      };
    case "thread/clearSelection":
      return {
        ...state,
        selectedThreadId: null,
        selectedThreadLoading: false,
      };
    case "thread/selected":
      return {
        ...state,
        selectedThreadId: action.thread.id,
        selectedThreadLoading: false,
        threads: upsertThread(state.threads, action.thread),
      };
    case "server/request":
      return {
        ...state,
        pendingRequests: [
          ...state.pendingRequests.filter(
            (request) =>
              requestIdKey(request.id) !== requestIdKey(action.request.id),
          ),
          action.request,
        ],
      };
    case "server/requestResolved":
      return {
        ...state,
        pendingRequests: state.pendingRequests.filter(
          (request) =>
            requestIdKey(request.id) !== requestIdKey(action.requestId),
        ),
      };
    case "server/notification":
      return applyNotification(state, action.notification);
    default:
      return state;
  }
}

export function setThreadsLoaded(threads: Thread[]): Action {
  return { type: "threads/loaded", threads };
}

export function setThreadSelected(thread: Thread): Action {
  return { type: "thread/selected", thread };
}

export function setConnectionStatus(
  status: ConnectionViewState,
  error?: string,
): Action {
  return { type: "connection/status", status, error };
}

export function setServerRequest(request: AnyServerRequest): Action {
  return { type: "server/request", request };
}

export function setServerNotification(
  notification: ServerNotification,
): Action {
  return { type: "server/notification", notification };
}

export function selectedThread(state: AppState): Thread | null {
  return (
    state.threads.find((thread) => thread.id === state.selectedThreadId) ?? null
  );
}

export function activeTurn(thread: Thread | null): Turn | null {
  if (!thread) {
    return null;
  }

  return (
    [...thread.turns].reverse().find((turn) => turn.status === "inProgress") ??
    null
  );
}

export function requestsForThread(
  state: AppState,
  threadId: string | null,
): AnyServerRequest[] {
  if (!threadId) {
    return [];
  }

  return state.pendingRequests.filter((request) => {
    const params = request.params;
    return (
      typeof params === "object" &&
      params !== null &&
      "threadId" in params &&
      params.threadId === threadId
    );
  });
}
