import type {
  AgentMessageDeltaNotification,
  AnyServerRequest,
  CommandExecutionOutputDeltaNotification,
  InitializeResponse,
  ItemCompletedNotification,
  ItemStartedNotification,
  PlanDeltaNotification,
  ReasoningSummaryPartAddedNotification,
  ReasoningSummaryTextDeltaNotification,
  ReasoningTextDeltaNotification,
  RequestId,
  ServerNotification,
  Thread,
  ThreadItem,
  ThreadStatus,
  Turn,
  TurnCompletedNotification,
  TurnStartedNotification,
} from "@/types/protocol";

import {
  CodexClient,
  type BackendMessage,
  type ThreadTurnsPage,
  type ThreadTurnsPageRequest,
} from "./codex-client";
import {
  PROXY_THREAD_LIST_UPDATED_METHOD,
  type ProxyThreadListSnapshot,
} from "./proxy-protocol";
import { requestIdKey } from "./request-id";

export type ConnectionViewState =
  | "idle"
  | "connecting"
  | "connected"
  | "disconnected"
  | "error";

export type ThreadListSnapshot = {
  connectionError: string | null;
  connectionState: ConnectionViewState;
  initializeSummary: string | null;
  loading: boolean;
  previewsByThreadId: Record<string, string>;
  threads: Thread[];
  warnings: string[];
};

export type ThreadDetailsSnapshot = {
  connectionError: string | null;
  connectionState: ConnectionViewState;
  initializeSummary: string | null;
  itemRuntimeText: Record<string, string>;
  loading: boolean;
  pendingRequests: AnyServerRequest[];
  thread: Thread | null;
  threadId: string;
  warnings: string[];
};

export interface BackendTransport {
  connect(): Promise<InitializeResponse>;
  disconnect(): void;
  interruptTurn(threadId: string, turnId: string): Promise<void>;
  listAllThreads(): Promise<{
    previewsByThreadId: Record<string, string>;
    threads: Thread[];
  }>;
  listThreadTurnsPage(
    threadId: string,
    request: ThreadTurnsPageRequest,
  ): Promise<ThreadTurnsPage>;
  respondToServerRequest(id: RequestId, response: unknown): void;
  renameThread(threadId: string, name: string): Promise<void>;
  resumeThread(threadId: string): Promise<Thread>;
  sendPrompt(threadId: string, text: string): Promise<void>;
  setForegroundThreadId(threadId: string | null): void;
  setPushSubscriptionEndpoint(endpoint: string | null): void;
  subscribe(listener: (message: BackendMessage) => void): () => void;
  subscribeStatus(
    listener: (status: string, error?: string) => void,
  ): () => void;
}

type ThreadSubscriber = (snapshot: ThreadDetailsSnapshot) => void;
type ThreadEntry = {
  initializing: Promise<void> | null;
  loading: boolean;
  snapshot: ThreadDetailsSnapshot;
  subscribers: Set<ThreadSubscriber>;
};

const EMPTY_REQUESTS: AnyServerRequest[] = [];
const EMPTY_THREAD_PREVIEWS: Record<string, string> = {};
const EMPTY_RUNTIME_TEXT: Record<string, string> = {};
const EMPTY_WARNINGS: string[] = [];
const RECONNECT_DELAY_MS = 1000;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function sortThreads(threads: Thread[]): Thread[] {
  return [...threads].sort((left, right) => right.updatedAt - left.updatedAt);
}

function stripThreadDetails(thread: Thread): Thread {
  if (thread.turns.length === 0) {
    return thread;
  }

  return {
    ...thread,
    turns: [],
  };
}

function upsertTurn(turns: Turn[], incoming: Turn): Turn[] {
  const index = turns.findIndex((turn) => turn.id === incoming.id);
  if (index === -1) {
    return [...turns, incoming];
  }

  const next = [...turns];
  const existingTurn = next[index]!;
  next[index] = {
    ...existingTurn,
    ...incoming,
    items: incoming.items.length > 0 ? incoming.items : existingTurn.items,
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

function updateThreadStatus(thread: Thread, status: ThreadStatus): Thread {
  return {
    ...thread,
    status,
    updatedAt: Math.max(thread.updatedAt, Math.floor(Date.now() / 1000)),
  };
}

function mergeTurn(existing: Turn | undefined, incoming: Turn): Turn {
  if (!existing) {
    return incoming;
  }

  return {
    ...existing,
    ...incoming,
    items: incoming.items.length > 0 ? incoming.items : existing.items,
  };
}

function hydratePagedThreadTurns(thread: Thread, turns: Turn[]): Turn[] {
  if (turns.length === 0) {
    return thread.turns;
  }

  const liveTurnsById = new Map(thread.turns.map((turn) => [turn.id, turn]));
  const mergedTurns = turns.map((turn) =>
    mergeTurn(liveTurnsById.get(turn.id), turn),
  );
  const latestPersistedTurnId = turns.at(-1)?.id;
  const latestLiveTurnIndex = thread.turns.findIndex(
    (turn) => turn.id === latestPersistedTurnId,
  );
  if (latestLiveTurnIndex === -1) {
    return mergedTurns;
  }

  const persistedTurnIds = new Set(turns.map((turn) => turn.id));
  for (const liveTurn of thread.turns.slice(latestLiveTurnIndex + 1)) {
    if (!persistedTurnIds.has(liveTurn.id)) {
      mergedTurns.push(liveTurn);
    }
  }

  return mergedTurns;
}

function appendUniqueWarning(warnings: string[], warning: string): string[] {
  return warnings.includes(warning) ? warnings : [...warnings, warning];
}

function applyTurnStarted(
  thread: Thread,
  notification: TurnStartedNotification,
): Thread {
  return {
    ...thread,
    status: { type: "active", activeFlags: [] },
    updatedAt: Math.floor(Date.now() / 1000),
    turns: upsertTurn(thread.turns, notification.turn),
  };
}

function applyTurnCompleted(
  thread: Thread,
  notification: TurnCompletedNotification,
): Thread {
  return {
    ...thread,
    status: thread.status.type === "active" ? { type: "idle" } : thread.status,
    updatedAt: Math.floor(Date.now() / 1000),
    turns: upsertTurn(thread.turns, notification.turn),
  };
}

function applyItemStarted(
  thread: Thread,
  notification: ItemStartedNotification,
): Thread {
  return updateTurn(thread, notification.turnId, (turn) => ({
    ...turn,
    items: upsertItem(turn.items, notification.item),
  }));
}

function applyItemCompleted(
  thread: Thread,
  notification: ItemCompletedNotification,
): Thread {
  return updateTurn(thread, notification.turnId, (turn) => ({
    ...turn,
    items: upsertItem(turn.items, notification.item),
  }));
}

function applyAgentDelta(
  thread: Thread,
  notification: AgentMessageDeltaNotification,
): Thread {
  return updateItem(
    thread,
    notification.turnId,
    notification.itemId,
    "agent",
    (item) =>
      item.type === "agentMessage"
        ? { ...item, text: `${item.text}${notification.delta}` }
        : item,
  );
}

function applyPlanDelta(
  thread: Thread,
  notification: PlanDeltaNotification,
): Thread {
  return updateItem(
    thread,
    notification.turnId,
    notification.itemId,
    "plan",
    (item) =>
      item.type === "plan"
        ? { ...item, text: `${item.text}${notification.delta}` }
        : item,
  );
}

function applyReasoningSummaryDelta(
  thread: Thread,
  notification: ReasoningSummaryTextDeltaNotification,
): Thread {
  return updateItem(
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
  );
}

function applyReasoningSummaryPart(
  thread: Thread,
  notification: ReasoningSummaryPartAddedNotification,
): Thread {
  return updateItem(
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
  );
}

function applyReasoningTextDelta(
  thread: Thread,
  notification: ReasoningTextDeltaNotification,
): Thread {
  return updateItem(
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
  );
}

function applyCommandOutputDelta(
  thread: Thread,
  notification: CommandExecutionOutputDeltaNotification,
): Thread {
  return updateItem(
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
  );
}

function getThreadIdFromValue(value: unknown): string | null {
  if (!isObject(value) || typeof value.threadId !== "string") {
    return null;
  }

  return value.threadId;
}

function listNotificationTouchesThread(
  notification: ServerNotification,
): string | null {
  switch (notification.method) {
    case "thread/started":
      return notification.params.thread.id;
    case "thread/status/changed":
    case "thread/archived":
    case "thread/unarchived":
    case "thread/closed":
    case "thread/name/updated":
    case "turn/started":
    case "turn/completed":
      return notification.params.threadId;
    default:
      return null;
  }
}

function applyDetailedNotification(
  thread: Thread,
  notification: ServerNotification,
): Thread {
  switch (notification.method) {
    case "thread/started":
      return notification.params.thread.id === thread.id
        ? notification.params.thread
        : thread;
    case "thread/status/changed":
      return notification.params.threadId === thread.id
        ? updateThreadStatus(thread, notification.params.status)
        : thread;
    case "thread/closed":
      return notification.params.threadId === thread.id
        ? updateThreadStatus(thread, { type: "notLoaded" })
        : thread;
    case "thread/name/updated":
      return notification.params.threadId === thread.id
        ? { ...thread, name: notification.params.threadName ?? null }
        : thread;
    case "turn/started":
      return notification.params.threadId === thread.id
        ? applyTurnStarted(thread, notification.params)
        : thread;
    case "turn/completed":
      return notification.params.threadId === thread.id
        ? applyTurnCompleted(thread, notification.params)
        : thread;
    case "item/started":
      return notification.params.threadId === thread.id
        ? applyItemStarted(thread, notification.params)
        : thread;
    case "item/completed":
      return notification.params.threadId === thread.id
        ? applyItemCompleted(thread, notification.params)
        : thread;
    case "item/agentMessage/delta":
      return notification.params.threadId === thread.id
        ? applyAgentDelta(thread, notification.params)
        : thread;
    case "item/plan/delta":
      return notification.params.threadId === thread.id
        ? applyPlanDelta(thread, notification.params)
        : thread;
    case "item/reasoning/summaryTextDelta":
      return notification.params.threadId === thread.id
        ? applyReasoningSummaryDelta(thread, notification.params)
        : thread;
    case "item/reasoning/summaryPartAdded":
      return notification.params.threadId === thread.id
        ? applyReasoningSummaryPart(thread, notification.params)
        : thread;
    case "item/reasoning/textDelta":
      return notification.params.threadId === thread.id
        ? applyReasoningTextDelta(thread, notification.params)
        : thread;
    case "item/commandExecution/outputDelta":
      return notification.params.threadId === thread.id
        ? applyCommandOutputDelta(thread, notification.params)
        : thread;
    default:
      return thread;
  }
}

export class BackendStateStore {
  private connectPromise: Promise<void> | null = null;
  private connectionError: string | null = null;
  private connectionState: ConnectionViewState = "idle";
  private initializeSummary: string | null = null;
  private readonly listSubscribers = new Set<
    (snapshot: ThreadListSnapshot) => void
  >();
  private listSyncPromise: Promise<void> | null = null;
  private listSnapshot: ThreadListSnapshot = {
    connectionError: null,
    connectionState: "idle",
    initializeSummary: null,
    loading: true,
    previewsByThreadId: EMPTY_THREAD_PREVIEWS,
    threads: [],
    warnings: [],
  };
  private readonly pendingRequestsByThread = new Map<
    string,
    AnyServerRequest[]
  >();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly runtimeTextByThread = new Map<
    string,
    Record<string, string>
  >();
  private readonly threadWarningsByThread = new Map<string, string[]>();
  private readonly previewByThreadId = new Map<string, string>();
  private readonly threadDetails = new Map<string, Thread>();
  private readonly threadEntries = new Map<string, ThreadEntry>();
  private threads: Thread[] = [];
  private threadsLoading = true;
  private globalWarnings: string[] = [];

  constructor(private readonly client: BackendTransport) {
    client.subscribe((message) => {
      if ("id" in message) {
        this.handleServerRequest(message);
        return;
      }

      if (message.method === PROXY_THREAD_LIST_UPDATED_METHOD) {
        this.handleProxyThreadListUpdated(message.params);
        return;
      }

      this.handleServerNotification(message);
    });

    client.subscribeStatus((status, error) => {
      this.handleTransportStatus(status, error);
    });
  }

  getThreadListSnapshot(): ThreadListSnapshot {
    return this.listSnapshot;
  }

  getThreadSnapshot(threadId: string): ThreadDetailsSnapshot {
    return this.ensureThreadEntry(threadId).snapshot;
  }

  subscribeThreadList(
    subscriber: (snapshot: ThreadListSnapshot) => void,
  ): () => void {
    this.listSubscribers.add(subscriber);
    subscriber(this.listSnapshot);
    void this.ensureConnected().catch(() => undefined);
    return () => {
      this.listSubscribers.delete(subscriber);
    };
  }

  subscribeThread(
    threadId: string,
    subscriber: (snapshot: ThreadDetailsSnapshot) => void,
  ): () => void {
    const entry = this.ensureThreadEntry(threadId);
    const firstSubscriber = entry.subscribers.size === 0;
    entry.subscribers.add(subscriber);
    if (firstSubscriber) {
      this.updateThreadEntry(threadId, { loading: true });
      void this.requestThreadLoad(threadId).catch(() => undefined);
    }

    subscriber(this.ensureThreadEntry(threadId).snapshot);
    return () => {
      const current = this.threadEntries.get(threadId);
      if (!current) {
        return;
      }

      current.subscribers.delete(subscriber);
      if (current.subscribers.size === 0) {
        this.threadEntries.delete(threadId);
        this.threadDetails.delete(threadId);
        this.runtimeTextByThread.delete(threadId);
      }
    };
  }

  async refreshThreadList(): Promise<void> {
    await this.ensureConnected();
    await this.reloadThreadList();
  }

  async sendPrompt(threadId: string, text: string): Promise<void> {
    await this.ensureConnected();
    await this.client.sendPrompt(threadId, text);
  }

  async renameThread(threadId: string, name: string): Promise<void> {
    await this.ensureConnected();
    await this.client.renameThread(threadId, name);
  }

  async interruptTurn(threadId: string, turnId: string): Promise<void> {
    await this.ensureConnected();
    await this.client.interruptTurn(threadId, turnId);
  }

  async respondToServerRequest(
    id: RequestId,
    response: unknown,
  ): Promise<void> {
    await this.ensureConnected();
    this.client.respondToServerRequest(id, response);
  }

  async setPushSubscriptionEndpoint(endpoint: string | null): Promise<void> {
    await this.ensureConnected();
    this.client.setPushSubscriptionEndpoint(endpoint);
  }

  async setForegroundThreadId(threadId: string | null): Promise<void> {
    this.client.setForegroundThreadId(threadId);
    await this.ensureConnected();
  }

  private ensureThreadEntry(threadId: string): ThreadEntry {
    const existing = this.threadEntries.get(threadId);
    if (existing) {
      return existing;
    }

    const entry: ThreadEntry = {
      initializing: null,
      loading: false,
      snapshot: this.buildThreadSnapshot(threadId, false),
      subscribers: new Set(),
    };
    this.threadEntries.set(threadId, entry);
    return entry;
  }

  private buildThreadSnapshot(
    threadId: string,
    loading: boolean,
  ): ThreadDetailsSnapshot {
    return {
      connectionError: this.connectionError,
      connectionState: this.connectionState,
      initializeSummary: this.initializeSummary,
      itemRuntimeText:
        this.runtimeTextByThread.get(threadId) ?? EMPTY_RUNTIME_TEXT,
      loading,
      pendingRequests:
        this.pendingRequestsByThread.get(threadId) ?? EMPTY_REQUESTS,
      thread:
        this.threadDetails.get(threadId) ??
        this.threads.find((thread) => thread.id === threadId) ??
        null,
      threadId,
      warnings: this.threadWarningsByThread.get(threadId) ?? EMPTY_WARNINGS,
    };
  }

  private updateListSnapshot(): void {
    const previewsByThreadId = Object.fromEntries(
      this.threads.flatMap((thread) => {
        const preview = this.previewByThreadId.get(thread.id);
        return preview ? [[thread.id, preview] as const] : [];
      }),
    );

    this.listSnapshot = {
      connectionError: this.connectionError,
      connectionState: this.connectionState,
      initializeSummary: this.initializeSummary,
      loading: this.threadsLoading,
      previewsByThreadId,
      threads: this.threads,
      warnings: this.globalWarnings,
    };

    for (const subscriber of this.listSubscribers) {
      subscriber(this.listSnapshot);
    }
  }

  private replaceListState(listState: ProxyThreadListSnapshot): void {
    this.previewByThreadId.clear();
    for (const [threadId, preview] of Object.entries(
      listState.previewsByThreadId,
    )) {
      this.previewByThreadId.set(threadId, preview);
    }
    this.threads = sortThreads(listState.threads.map(stripThreadDetails));
  }

  private updateThreadEntry(
    threadId: string,
    options: { loading?: boolean } = {},
  ): void {
    const entry = this.ensureThreadEntry(threadId);
    if (options.loading !== undefined) {
      entry.loading = options.loading;
    }
    entry.snapshot = this.buildThreadSnapshot(threadId, entry.loading);
    for (const subscriber of entry.subscribers) {
      subscriber(entry.snapshot);
    }
  }

  private updateAllThreadEntries(): void {
    for (const threadId of this.threadEntries.keys()) {
      this.updateThreadEntry(threadId);
    }
  }

  private updateConnectionState(
    state: ConnectionViewState,
    error?: string | null,
  ): void {
    this.connectionState = state;
    if (error !== undefined) {
      this.connectionError = error;
    }
    this.updateListSnapshot();
    this.updateAllThreadEntries();
  }

  private handleProxyThreadListUpdated(
    listState: ProxyThreadListSnapshot,
  ): void {
    this.replaceListState(listState);
    this.updateListSnapshot();
    this.updateAllThreadEntries();
  }

  private handleTransportStatus(status: string, error?: string): void {
    switch (status) {
      case "connecting":
        this.updateConnectionState("connecting");
        return;
      case "open":
        return;
      case "error":
        this.updateConnectionState(
          "error",
          error ?? "websocket transport error",
        );
        this.scheduleReconnect();
        return;
      case "closed":
        this.updateConnectionState(
          "disconnected",
          error ?? "connection closed",
        );
        this.scheduleReconnect();
        return;
      case "idle":
        this.updateConnectionState("idle");
        return;
      default:
        return;
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer !== null || this.connectPromise !== null) {
      return;
    }

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.ensureConnected().catch(() => undefined);
    }, RECONNECT_DELAY_MS);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer === null) {
      return;
    }

    clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }

  private async ensureConnected(): Promise<void> {
    if (this.connectionState === "connected") {
      return;
    }

    if (this.connectPromise) {
      return this.connectPromise;
    }

    this.clearReconnectTimer();
    this.connectPromise = (async () => {
      try {
        const init = await this.client.connect();
        this.initializeSummary = `${init.userAgent} on ${init.platformOs}`;
        this.connectionError = null;
        this.connectionState = "connected";
        this.updateListSnapshot();
        this.updateAllThreadEntries();
        await this.reloadThreadList();

        for (const [threadId, entry] of this.threadEntries.entries()) {
          if (entry.initializing === null) {
            void this.requestThreadLoad(threadId).catch(() => undefined);
          }
        }
      } finally {
        this.connectPromise = null;
      }
    })();

    return this.connectPromise;
  }

  private async reloadThreadList(): Promise<void> {
    if (this.listSyncPromise) {
      return this.listSyncPromise;
    }

    this.threadsLoading = true;
    this.updateListSnapshot();
    this.listSyncPromise = (async () => {
      try {
        const { previewsByThreadId, threads } =
          await this.client.listAllThreads();
        this.replaceListState({
          previewsByThreadId,
          threads,
        });
      } finally {
        this.threadsLoading = false;
        this.updateListSnapshot();
        this.updateAllThreadEntries();
        this.listSyncPromise = null;
      }
    })();

    return this.listSyncPromise;
  }

  private async requestThreadLoad(threadId: string): Promise<void> {
    const entry = this.ensureThreadEntry(threadId);
    if (entry.initializing) {
      return entry.initializing;
    }

    entry.initializing = (async () => {
      let persistedTurns: Turn[] = [];
      let nextTurnsCursor: string | null = null;

      try {
        await this.ensureConnected();
        try {
          const initialTurnsPage = await this.client.listThreadTurnsPage(
            threadId,
            {
              cursor: null,
              sortDirection: "desc",
            },
          );
          persistedTurns = initialTurnsPage.turns;
          nextTurnsCursor = initialTurnsPage.nextCursor;
        } catch {
          persistedTurns = [];
          nextTurnsCursor = null;
        }

        const listedThread = this.threads.find(
          (thread) => thread.id === threadId,
        );
        if (listedThread && persistedTurns.length > 0) {
          const current = this.threadEntries.get(threadId);
          if (current) {
            this.threadDetails.set(threadId, {
              ...listedThread,
              turns: persistedTurns,
            });
            this.updateThreadEntry(threadId);
          }
        }

        const liveThread = await this.client.resumeThread(threadId);
        let thread =
          persistedTurns.length > 0
            ? {
                ...liveThread,
                turns: hydratePagedThreadTurns(liveThread, persistedTurns),
              }
            : liveThread;

        const current = this.threadEntries.get(threadId);
        if (current) {
          this.threadDetails.set(threadId, thread);
          current.loading = false;
          this.updateThreadEntry(threadId);
        }

        while (nextTurnsCursor) {
          const page = await this.client.listThreadTurnsPage(threadId, {
            cursor: nextTurnsCursor,
            sortDirection: "desc",
          });
          persistedTurns = [...page.turns, ...persistedTurns];
          nextTurnsCursor = page.nextCursor;

          const current = this.threadEntries.get(threadId);
          if (!current) {
            return;
          }

          const currentThread = this.threadDetails.get(threadId) ?? thread;
          thread = {
            ...currentThread,
            turns: hydratePagedThreadTurns(currentThread, persistedTurns),
          };
          this.threadDetails.set(threadId, thread);
          current.loading = false;
          this.updateThreadEntry(threadId);
        }
      } finally {
        const current = this.threadEntries.get(threadId);
        if (current) {
          current.initializing = null;
          current.loading = false;
          this.updateThreadEntry(threadId);
        }
      }
    })();

    return entry.initializing;
  }

  private handleServerRequest(request: AnyServerRequest): void {
    const threadId = getThreadIdFromValue(request.params);
    if (!threadId) {
      return;
    }

    const existing =
      this.pendingRequestsByThread.get(threadId) ?? EMPTY_REQUESTS;
    const next = [
      ...existing.filter(
        (current) => requestIdKey(current.id) !== requestIdKey(request.id),
      ),
      request,
    ];
    this.pendingRequestsByThread.set(threadId, next);
    if (this.threadEntries.has(threadId)) {
      this.updateThreadEntry(threadId);
    }
  }

  private handleServerNotification(notification: ServerNotification): void {
    if (notification.method === "error") {
      this.connectionError = notification.params.error.message;
      this.updateListSnapshot();
      this.updateAllThreadEntries();
      return;
    }

    if (notification.method === "serverRequest/resolved") {
      const threadId = notification.params.threadId;
      const existing =
        this.pendingRequestsByThread.get(threadId) ?? EMPTY_REQUESTS;
      const next = existing.filter(
        (request) =>
          requestIdKey(request.id) !==
          requestIdKey(notification.params.requestId),
      );
      if (next.length === 0) {
        this.pendingRequestsByThread.delete(threadId);
      } else {
        this.pendingRequestsByThread.set(threadId, next);
      }
      if (this.threadEntries.has(threadId)) {
        this.updateThreadEntry(threadId);
      }
      return;
    }

    if (notification.method === "warning") {
      const { message, threadId } = notification.params;
      if (threadId) {
        const warnings =
          this.threadWarningsByThread.get(threadId) ?? EMPTY_WARNINGS;
        this.threadWarningsByThread.set(
          threadId,
          appendUniqueWarning(warnings, message),
        );
        if (this.threadEntries.has(threadId)) {
          this.updateThreadEntry(threadId);
        }
        return;
      }

      this.globalWarnings = appendUniqueWarning(this.globalWarnings, message);
      this.updateListSnapshot();
      return;
    }

    const threadId =
      listNotificationTouchesThread(notification) ??
      getThreadIdFromValue(notification.params);
    if (!threadId) {
      return;
    }

    if (notification.method === "thread/archived") {
      this.threadDetails.delete(threadId);
      this.runtimeTextByThread.delete(threadId);
      this.threadWarningsByThread.delete(threadId);
    } else {
      const existingThread = this.threadDetails.get(threadId);
      if (existingThread) {
        this.threadDetails.set(
          threadId,
          applyDetailedNotification(existingThread, notification),
        );
      }
    }

    if (notification.method === "item/fileChange/outputDelta") {
      if (this.threadEntries.has(threadId)) {
        const existing =
          this.runtimeTextByThread.get(threadId) ?? EMPTY_RUNTIME_TEXT;
        this.runtimeTextByThread.set(threadId, {
          ...existing,
          [notification.params.itemId]:
            `${existing[notification.params.itemId] ?? ""}${notification.params.delta}`,
        });
      }
    }

    if (
      notification.method === "item/completed" &&
      this.threadEntries.has(threadId)
    ) {
      const existing =
        this.runtimeTextByThread.get(threadId) ?? EMPTY_RUNTIME_TEXT;
      this.runtimeTextByThread.set(threadId, {
        ...existing,
        [notification.params.item.id]: "",
      });
    }

    if (this.threadEntries.has(threadId)) {
      this.updateThreadEntry(threadId);
    }
  }
}

const backendStore = new BackendStateStore(new CodexClient("/rpc"));

export function getThreadListSnapshot(): ThreadListSnapshot {
  return backendStore.getThreadListSnapshot();
}

export function subscribeThreadList(
  subscriber: (snapshot: ThreadListSnapshot) => void,
): () => void {
  return backendStore.subscribeThreadList(subscriber);
}

export function getThreadSnapshot(threadId: string): ThreadDetailsSnapshot {
  return backendStore.getThreadSnapshot(threadId);
}

export function subscribeThread(
  threadId: string,
  subscriber: (snapshot: ThreadDetailsSnapshot) => void,
): () => void {
  return backendStore.subscribeThread(threadId, subscriber);
}

export function refreshThreadList(): Promise<void> {
  return backendStore.refreshThreadList();
}

export function sendPrompt(threadId: string, text: string): Promise<void> {
  return backendStore.sendPrompt(threadId, text);
}

export function renameThread(threadId: string, name: string): Promise<void> {
  return backendStore.renameThread(threadId, name);
}

export function interruptTurn(threadId: string, turnId: string): Promise<void> {
  return backendStore.interruptTurn(threadId, turnId);
}

export function respondToServerRequest(
  id: RequestId,
  response: unknown,
): Promise<void> {
  return backendStore.respondToServerRequest(id, response);
}

export function setPushSubscriptionEndpoint(
  endpoint: string | null,
): Promise<void> {
  return backendStore.setPushSubscriptionEndpoint(endpoint);
}

export function setForegroundThreadId(threadId: string | null): Promise<void> {
  return backendStore.setForegroundThreadId(threadId);
}
