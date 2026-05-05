import { waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { InitializeResponse, Thread, Turn } from "@/types/protocol";

import { BackendStateStore, type BackendTransport } from "./backend-store";
import type { BackendMessage } from "./codex-client";
import type { ThreadTurnsPage, ThreadTurnsPageRequest } from "./codex-client";
import { PROXY_THREAD_LIST_UPDATED_METHOD } from "./proxy-protocol";

type MessageListener = (message: BackendMessage) => void;
type StatusListener = (
  status: "idle" | "connecting" | "open" | "closed" | "error",
  error?: string,
) => void;

function makeThread(id: string, name = id): Thread {
  return {
    id,
    forkedFromId: null,
    preview: `${name} preview`,
    ephemeral: false,
    modelProvider: "openai",
    createdAt: 1,
    updatedAt: 1,
    status: { type: "idle" },
    path: null,
    cwd: "/tmp/project",
    cliVersion: "0.0.0",
    source: "appServer",
    agentNickname: null,
    agentRole: null,
    gitInfo: null,
    name,
    turns: [],
  };
}

function makeTurn(id: string): Turn {
  return {
    id,
    items: [],
    status: "completed",
    error: null,
    startedAt: 1,
    completedAt: 2,
    durationMs: 1000,
  };
}

function flushPromises(): Promise<void> {
  return new Promise((resolve) => {
    queueMicrotask(() => resolve());
  });
}

class FakeTransport implements BackendTransport {
  connectCalls = 0;
  listCalls = 0;
  listTurnsPageCalls: Array<{
    request: ThreadTurnsPageRequest;
    threadId: string;
  }> = [];
  resumeThreadGate: Promise<void> | null = null;
  resumeCalls = new Map<string, number>();
  renameThreadCalls: Array<{ name: string; threadId: string }> = [];
  private readonly messageListeners = new Set<MessageListener>();
  private readonly statusListeners = new Set<StatusListener>();

  constructor(
    private readonly threads: Thread[],
    private readonly previewsByThreadId: Record<string, string> = {},
    private readonly detailedThreads = new Map<string, Thread>(),
    private readonly turnPagesByThread = new Map<string, ThreadTurnsPage[]>(),
  ) {}

  async connect(): Promise<InitializeResponse> {
    this.connectCalls += 1;
    this.emitStatus("connecting");
    this.emitStatus("open");
    return {
      userAgent: "codex-test",
      codexHome: "/tmp/codex",
      platformFamily: "unix",
      platformOs: "linux",
    };
  }

  disconnect(): void {}

  async interruptTurn(): Promise<void> {}

  async listAllThreads(): Promise<{
    previewsByThreadId: Record<string, string>;
    threads: Thread[];
  }> {
    this.listCalls += 1;
    return {
      previewsByThreadId: { ...this.previewsByThreadId },
      threads: this.threads.map((thread) => ({ ...thread })),
    };
  }

  async listThreadTurnsPage(
    threadId: string,
    request: ThreadTurnsPageRequest,
  ): Promise<ThreadTurnsPage> {
    this.listTurnsPageCalls.push({ request, threadId });
    const pages = this.turnPagesByThread.get(threadId);
    const page = pages?.shift();
    return {
      nextCursor: page?.nextCursor ?? null,
      turns: [...(page?.turns ?? [])],
    };
  }

  respondToServerRequest(): void {}

  async renameThread(threadId: string, name: string): Promise<void> {
    this.renameThreadCalls.push({ name, threadId });
  }

  async resumeThread(threadId: string): Promise<Thread> {
    this.resumeCalls.set(threadId, (this.resumeCalls.get(threadId) ?? 0) + 1);
    await this.resumeThreadGate;
    return {
      ...(this.detailedThreads.get(threadId) ?? makeThread(threadId)),
    };
  }

  async sendPrompt(): Promise<void> {}

  subscribe(listener: MessageListener): () => void {
    this.messageListeners.add(listener);
    return () => {
      this.messageListeners.delete(listener);
    };
  }

  subscribeStatus(listener: StatusListener): () => void {
    this.statusListeners.add(listener);
    listener("idle");
    return () => {
      this.statusListeners.delete(listener);
    };
  }

  emitMessage(message: BackendMessage): void {
    for (const listener of this.messageListeners) {
      listener(message);
    }
  }

  private emitStatus(
    status: Parameters<StatusListener>[0],
    error?: string,
  ): void {
    for (const listener of this.statusListeners) {
      listener(status, error);
    }
  }
}

describe("BackendStateStore", () => {
  it("connects and loads the thread list only once for multiple subscribers", async () => {
    const transport = new FakeTransport([makeThread("thr_1", "Thread 1")]);
    const store = new BackendStateStore(transport);
    const first = vi.fn();
    const second = vi.fn();

    const unsubscribeFirst = store.subscribeThreadList(first);
    const unsubscribeSecond = store.subscribeThreadList(second);
    await flushPromises();
    await flushPromises();

    expect(transport.connectCalls).toBe(1);
    expect(transport.listCalls).toBe(1);
    expect(first).toHaveBeenLastCalledWith(
      expect.objectContaining({
        connectionState: "connected",
        threads: [expect.objectContaining({ id: "thr_1" })],
      }),
    );
    expect(second).toHaveBeenLastCalledWith(
      expect.objectContaining({
        connectionState: "connected",
        threads: [expect.objectContaining({ id: "thr_1" })],
      }),
    );

    unsubscribeFirst();
    unsubscribeSecond();
  });

  it("applies proxy thread-list updates without reloading the full list", async () => {
    const transport = new FakeTransport([makeThread("thr_1", "Thread 1")]);
    const store = new BackendStateStore(transport);
    const subscriber = vi.fn();

    store.subscribeThreadList(subscriber);
    await flushPromises();
    await flushPromises();

    transport.emitMessage({
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: {
        previewsByThreadId: {},
        threads: [
          makeThread("thr_1", "Thread 1"),
          makeThread("thr_2", "Thread 2"),
        ],
      },
    });

    expect(transport.listCalls).toBe(1);
    expect(subscriber).toHaveBeenLastCalledWith(
      expect.objectContaining({
        threads: [
          expect.objectContaining({ id: "thr_1" }),
          expect.objectContaining({ id: "thr_2" }),
        ],
      }),
    );
  });

  it("hydrates thread list previews from the latest user or agent message", async () => {
    const transport = new FakeTransport(
      [makeThread("thr_1", "Thread 1"), makeThread("thr_2", "Thread 2")],
      {
        thr_1: "**Codex:** latest response",
        thr_2: "**You:** latest prompt",
      },
    );
    const store = new BackendStateStore(transport);
    const subscriber = vi.fn();

    store.subscribeThreadList(subscriber);

    await waitFor(() => {
      expect(subscriber).toHaveBeenLastCalledWith(
        expect.objectContaining({
          previewsByThreadId: {
            thr_1: "**Codex:** latest response",
            thr_2: "**You:** latest prompt",
          },
        }),
      );
    });
  });

  it("updates the thread list preview from proxy snapshots", async () => {
    const transport = new FakeTransport([makeThread("thr_1", "Thread 1")]);
    const store = new BackendStateStore(transport);
    const subscriber = vi.fn();

    store.subscribeThreadList(subscriber);
    await flushPromises();
    await flushPromises();

    transport.emitMessage({
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: {
        previewsByThreadId: {
          thr_1: "**Codex:** Working on it",
        },
        threads: [makeThread("thr_1", "Thread 1")],
      },
    });

    expect(subscriber).toHaveBeenLastCalledWith(
      expect.objectContaining({
        previewsByThreadId: {
          thr_1: "**Codex:** Working on it",
        },
      }),
    );
  });

  it("loads thread details only once for multiple subscribers to the same thread", async () => {
    const detailedThread: Thread = makeThread("thr_1", "Thread 1");
    const persistedTurns = [makeTurn("turn_1")];
    const transport = new FakeTransport(
      [makeThread("thr_1", "Thread 1")],
      {},
      new Map([["thr_1", detailedThread]]),
      new Map([
        [
          "thr_1",
          [
            {
              nextCursor: null,
              turns: persistedTurns,
            },
          ],
        ],
      ]),
    );
    const store = new BackendStateStore(transport);
    const first = vi.fn();
    const second = vi.fn();

    const unsubscribeFirst = store.subscribeThread("thr_1", first);
    const unsubscribeSecond = store.subscribeThread("thr_1", second);
    await waitFor(() => {
      expect(transport.connectCalls).toBe(1);
      expect(transport.resumeCalls.get("thr_1")).toBe(1);
      expect(transport.listTurnsPageCalls).toEqual([
        {
          request: { cursor: null, sortDirection: "desc" },
          threadId: "thr_1",
        },
      ]);
      expect(first).toHaveBeenLastCalledWith(
        expect.objectContaining({
          loading: false,
          thread: expect.objectContaining({
            id: "thr_1",
            turns: [expect.objectContaining({ id: "turn_1" })],
          }),
        }),
      );
      expect(second).toHaveBeenLastCalledWith(
        expect.objectContaining({
          loading: false,
          thread: expect.objectContaining({
            id: "thr_1",
            turns: [expect.objectContaining({ id: "turn_1" })],
          }),
        }),
      );
    });

    unsubscribeFirst();
    unsubscribeSecond();
  });

  it("renders the newest turn page before loading older history", async () => {
    let releaseResume: () => void = () => {
      throw new Error("resume gate was not initialized");
    };
    const listedThread = makeThread("thr_1", "Thread 1");
    const transport = new FakeTransport(
      [listedThread],
      {},
      new Map([
        [
          "thr_1",
          {
            ...listedThread,
            turns: [makeTurn("turn_1"), makeTurn("turn_2"), makeTurn("turn_3")],
          },
        ],
      ]),
      new Map([
        [
          "thr_1",
          [
            {
              nextCursor: "older",
              turns: [makeTurn("turn_2"), makeTurn("turn_3")],
            },
            {
              nextCursor: null,
              turns: [makeTurn("turn_1")],
            },
          ],
        ],
      ]),
    );
    transport.resumeThreadGate = new Promise((resolve) => {
      releaseResume = resolve;
    });
    const store = new BackendStateStore(transport);
    const subscriber = vi.fn();

    store.subscribeThread("thr_1", subscriber);

    await waitFor(() => {
      expect(subscriber).toHaveBeenLastCalledWith(
        expect.objectContaining({
          thread: expect.objectContaining({
            turns: [
              expect.objectContaining({ id: "turn_2" }),
              expect.objectContaining({ id: "turn_3" }),
            ],
          }),
        }),
      );
    });
    expect(transport.resumeCalls.get("thr_1")).toBe(1);
    releaseResume();

    await waitFor(() => {
      expect(subscriber).toHaveBeenLastCalledWith(
        expect.objectContaining({
          loading: false,
          thread: expect.objectContaining({
            turns: [
              expect.objectContaining({ id: "turn_1" }),
              expect.objectContaining({ id: "turn_2" }),
              expect.objectContaining({ id: "turn_3" }),
            ],
          }),
        }),
      );
    });
    expect(transport.listTurnsPageCalls).toEqual([
      {
        request: { cursor: null, sortDirection: "desc" },
        threadId: "thr_1",
      },
      {
        request: { cursor: "older", sortDirection: "desc" },
        threadId: "thr_1",
      },
    ]);
  });

  it("surfaces global and thread warnings separately", async () => {
    const transport = new FakeTransport([makeThread("thr_1", "Thread 1")]);
    const store = new BackendStateStore(transport);
    const listSubscriber = vi.fn();
    const threadSubscriber = vi.fn();

    store.subscribeThreadList(listSubscriber);
    store.subscribeThread("thr_1", threadSubscriber);
    await flushPromises();
    await flushPromises();

    transport.emitMessage({
      method: "warning",
      params: {
        threadId: null,
        message: "Global warning",
      },
    });
    transport.emitMessage({
      method: "warning",
      params: {
        threadId: "thr_1",
        message: "Thread warning",
      },
    });

    expect(listSubscriber).toHaveBeenLastCalledWith(
      expect.objectContaining({
        warnings: ["Global warning"],
      }),
    );
    expect(threadSubscriber).toHaveBeenLastCalledWith(
      expect.objectContaining({
        warnings: ["Thread warning"],
      }),
    );
  });

  it("sends thread renames without mutating title state until the server update", async () => {
    const thread = makeThread("thr_1", "Old thread");
    const transport = new FakeTransport(
      [thread],
      {},
      new Map([["thr_1", thread]]),
    );
    const store = new BackendStateStore(transport);
    const subscriber = vi.fn();

    store.subscribeThread("thr_1", subscriber);

    await waitFor(() => {
      expect(subscriber).toHaveBeenLastCalledWith(
        expect.objectContaining({
          thread: expect.objectContaining({ name: "Old thread" }),
        }),
      );
    });

    await store.renameThread("thr_1", "New thread");

    expect(transport.renameThreadCalls).toEqual([
      { name: "New thread", threadId: "thr_1" },
    ]);
    expect(subscriber).toHaveBeenLastCalledWith(
      expect.objectContaining({
        thread: expect.objectContaining({ name: "Old thread" }),
      }),
    );

    transport.emitMessage({
      method: "thread/name/updated",
      params: {
        threadId: "thr_1",
        threadName: "New thread",
      },
    });

    expect(subscriber).toHaveBeenLastCalledWith(
      expect.objectContaining({
        thread: expect.objectContaining({ name: "New thread" }),
      }),
    );
  });
});
