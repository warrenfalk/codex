import type {
  AgentMessageDeltaNotification,
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
} from "../src/types/protocol";

import type {
  ProxyThreadActivity,
  ProxyThreadListSnapshot,
} from "../src/lib/proxy-protocol";

type AgentMessageItem = Extract<ThreadItem, { type: "agentMessage" }>;
type UserMessageItem = Extract<ThreadItem, { type: "userMessage" }>;
const TURN_ITEMS_VIEW_RANK: Record<Turn["itemsView"], number> = {
  notLoaded: 0,
  summary: 1,
  full: 2,
};

function sortThreads(threads: Thread[]): Thread[] {
  return [...threads].sort((left, right) => right.updatedAt - left.updatedAt);
}

function trimPreviewMarkdown(text: string): string {
  return text.trim();
}

function isNonEmptyAgentMessage(item: ThreadItem): item is AgentMessageItem {
  return item.type === "agentMessage" && Boolean(item.text.trim());
}

function isUserMessage(item: ThreadItem): item is UserMessageItem {
  return item.type === "userMessage";
}

function userMessageText(item: UserMessageItem): string | null {
  const text = trimPreviewMarkdown(
    item.content
      .flatMap((entry) => (entry.type === "text" ? [entry.text] : []))
      .join("\n\n"),
  );
  return text || null;
}

function previewFromItem(item: ThreadItem): string | null {
  switch (item.type) {
    case "userMessage": {
      const text = userMessageText(item);
      return text ? `**You:** ${text}` : "**You:** Sent input";
    }
    case "agentMessage": {
      const text = trimPreviewMarkdown(item.text);
      return text ? `**Codex:** ${text}` : null;
    }
    default:
      return null;
  }
}

function previewFromTurns(turns: Turn[]): string | null {
  for (let turnIndex = turns.length - 1; turnIndex >= 0; turnIndex -= 1) {
    const turn = turns[turnIndex];
    if (!turn) {
      continue;
    }

    for (
      let itemIndex = turn.items.length - 1;
      itemIndex >= 0;
      itemIndex -= 1
    ) {
      const item = turn.items[itemIndex];
      if (!item) {
        continue;
      }

      const preview = previewFromItem(item);
      if (preview) {
        return preview;
      }
    }
  }

  return null;
}

function latestKnownTurn(turns: Turn[]): Turn | null {
  let latest: Turn | null = null;
  for (const turn of turns) {
    if (!latest || turnSortValue(turn) >= turnSortValue(latest)) {
      latest = turn;
    }
  }
  return latest;
}

function turnSortValue(turn: Turn): number {
  return turn.startedAt ?? turn.completedAt ?? 0;
}

function latestUserMessageFromTurn(turn: Turn): string | null {
  for (let index = turn.items.length - 1; index >= 0; index -= 1) {
    const item = turn.items[index];
    if (item && isUserMessage(item)) {
      return userMessageText(item);
    }
  }
  return null;
}

function latestAgentMessageFromTurn(turn: Turn): string | null {
  for (let index = turn.items.length - 1; index >= 0; index -= 1) {
    const item = turn.items[index];
    if (item && isNonEmptyAgentMessage(item)) {
      return item.text.trim();
    }
  }
  return null;
}

function activityFromThread(thread: Thread): ProxyThreadActivity {
  const latestTurn = latestKnownTurn(thread.turns);
  const lastUserMessage = latestTurn
    ? latestUserMessageFromTurn(latestTurn)
    : null;
  const lastAgentMessage = latestTurn
    ? latestAgentMessageFromTurn(latestTurn)
    : null;

  if (thread.status.type === "active" || latestTurn?.status === "inProgress") {
    return {
      lastAgentMessage,
      lastUserMessage,
      state: "working",
    };
  }

  if (!lastUserMessage && thread.turns.length === 0 && !thread.preview.trim()) {
    return {
      lastAgentMessage,
      lastUserMessage,
      state: "virgin",
    };
  }

  return {
    lastAgentMessage,
    lastUserMessage,
    state: "ready",
  };
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

function updateThreadStatus(thread: Thread, status: ThreadStatus): Thread {
  return {
    ...thread,
    status,
    updatedAt: Math.max(thread.updatedAt, Math.floor(Date.now() / 1000)),
  };
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

function updatedAtNow(thread: Thread): Thread {
  return {
    ...thread,
    updatedAt: Math.floor(Date.now() / 1000),
  };
}

export class ThreadCache {
  private readonly activityByThreadId = new Map<string, ProxyThreadActivity>();
  private readonly previewsByThreadId = new Map<string, string>();
  private readonly threadsById = new Map<string, Thread>();

  replaceThreads(threads: Thread[]): boolean {
    for (const thread of threads) {
      this.threadsById.set(
        thread.id,
        mergeThread(this.threadsById.get(thread.id), thread),
      );
    }
    return this.rebuildPreviews();
  }

  clear(): void {
    this.activityByThreadId.clear();
    this.previewsByThreadId.clear();
    this.threadsById.clear();
  }

  mergeThread(thread: Thread): boolean {
    this.threadsById.set(
      thread.id,
      mergeThread(this.threadsById.get(thread.id), thread),
    );
    return this.rebuildPreviews();
  }

  mergeThreadTurns(threadId: string, turns: Turn[]): boolean {
    const thread = this.threadsById.get(threadId);
    if (!thread || turns.length === 0) {
      return false;
    }

    this.threadsById.set(
      thread.id,
      turns.reduce(
        (current, turn) => ({
          ...current,
          turns: upsertTurn(current.turns, turn),
        }),
        thread,
      ),
    );
    return this.rebuildPreviews();
  }

  latestAgentMessageForTurn(threadId: string, turnId: string): string | null {
    const turn = this.threadsById
      .get(threadId)
      ?.turns.find((candidate) => candidate.id === turnId);
    if (!turn) {
      return null;
    }

    const messages = turn.items.filter(isNonEmptyAgentMessage);
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      const message = messages[index]!;
      if (message.phase === "final_answer" || message.phase === null) {
        return message.text.trim();
      }
    }

    return messages.at(-1)?.text.trim() ?? null;
  }

  applyNotification(notification: ServerNotification): boolean {
    let changed = false;

    switch (notification.method) {
      case "thread/started": {
        this.threadsById.set(
          notification.params.thread.id,
          notification.params.thread,
        );
        return this.rebuildPreviews();
      }
      case "thread/status/changed": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updateThreadStatus(thread, notification.params.status),
        );
        return true;
      }
      case "thread/archived": {
        changed =
          this.threadsById.delete(notification.params.threadId) || changed;
        changed =
          this.previewsByThreadId.delete(notification.params.threadId) ||
          changed;
        return changed;
      }
      case "thread/closed": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updateThreadStatus(thread, { type: "notLoaded" }),
        );
        return true;
      }
      case "thread/name/updated": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(thread.id, {
          ...thread,
          name: notification.params.threadName ?? null,
          updatedAt: Math.floor(Date.now() / 1000),
        });
        return true;
      }
      case "turn/started": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          applyTurnStarted(thread, notification.params),
        );
        return this.rebuildPreviews();
      }
      case "turn/completed": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          applyTurnCompleted(thread, notification.params),
        );
        return this.rebuildPreviews();
      }
      case "item/started": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyItemStarted(thread, notification.params)),
        );
        return this.rebuildPreviews();
      }
      case "item/completed": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyItemCompleted(thread, notification.params)),
        );
        return this.rebuildPreviews();
      }
      case "item/agentMessage/delta": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyAgentDelta(thread, notification.params)),
        );
        return this.rebuildPreviews();
      }
      case "item/plan/delta": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyPlanDelta(thread, notification.params)),
        );
        return false;
      }
      case "item/reasoning/summaryTextDelta": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyReasoningSummaryDelta(thread, notification.params)),
        );
        return false;
      }
      case "item/reasoning/summaryPartAdded": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyReasoningSummaryPart(thread, notification.params)),
        );
        return false;
      }
      case "item/reasoning/textDelta": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyReasoningTextDelta(thread, notification.params)),
        );
        return false;
      }
      case "item/commandExecution/outputDelta": {
        const thread = this.threadsById.get(notification.params.threadId);
        if (!thread) {
          return false;
        }
        this.threadsById.set(
          thread.id,
          updatedAtNow(applyCommandOutputDelta(thread, notification.params)),
        );
        return false;
      }
      default:
        return false;
    }
  }

  snapshot(): ProxyThreadListSnapshot {
    const threads = sortThreads(
      [...this.threadsById.values()].map(stripThreadDetails),
    );
    return {
      threadActivityByThreadId: Object.fromEntries(this.activityByThreadId),
      previewsByThreadId: Object.fromEntries(this.previewsByThreadId),
      threads,
    };
  }

  private rebuildPreviews(): boolean {
    const nextActivity = new Map<string, ProxyThreadActivity>();
    const nextPreviews = new Map<string, string>();
    for (const thread of this.threadsById.values()) {
      nextActivity.set(thread.id, activityFromThread(thread));
      const preview = previewFromTurns(thread.turns);
      if (preview) {
        nextPreviews.set(thread.id, preview);
      }
    }

    this.activityByThreadId.clear();
    for (const [threadId, activity] of nextActivity) {
      this.activityByThreadId.set(threadId, activity);
    }
    this.previewsByThreadId.clear();
    for (const [threadId, preview] of nextPreviews) {
      this.previewsByThreadId.set(threadId, preview);
    }
    return true;
  }
}
