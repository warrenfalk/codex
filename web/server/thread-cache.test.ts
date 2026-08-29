import { describe, expect, it } from "vitest";

import type { Thread } from "../src/types/protocol";

import { ThreadCache } from "./thread-cache.js";

function buildThread(id: string, overrides: Partial<Thread> = {}): Thread {
  return {
    id,
    sessionId: id,
    forkedFromId: null,
    parentThreadId: null,
    preview: `${id} preview`,
    projectId: null,
    ephemeral: false,
    section: null,
    sectionEnteredAt: null,
    modelProvider: "openai",
    createdAt: 1,
    updatedAt: 1,
    recencyAt: 1,
    status: { type: "idle" },
    path: null,
    cwd: "/tmp/project",
    cliVersion: "0.0.0",
    source: "appServer",
    threadSource: null,
    agentNickname: null,
    agentRole: null,
    gitInfo: null,
    name: id,
    turns: [],
    ...overrides,
  };
}

describe("ThreadCache", () => {
  it("orders snapshot threads by recency timestamp", () => {
    const cache = new ThreadCache();

    cache.replaceThreads([
      buildThread("updated-newer", { updatedAt: 30, recencyAt: 1 }),
      buildThread("recent-newer", { updatedAt: 2, recencyAt: 40 }),
    ]);

    expect(cache.snapshot().threads.map((thread) => thread.id)).toEqual([
      "recent-newer",
      "updated-newer",
    ]);
  });

  it("removes deleted threads from snapshots", () => {
    const cache = new ThreadCache();
    cache.replaceThreads([buildThread("thr_1"), buildThread("thr_2")]);

    const changed = cache.applyNotification({
      method: "thread/deleted",
      params: {
        threadId: "thr_1",
      },
    });

    expect(changed).toBe(true);
    expect(cache.snapshot().threads.map((thread) => thread.id)).toEqual([
      "thr_2",
    ]);
  });

  it("invalidates cached turns when a thread is reverted", () => {
    const cache = new ThreadCache();
    cache.replaceThreads([
      buildThread("thr_1", {
        turns: [
          {
            completedAt: 2,
            durationMs: 1000,
            error: null,
            id: "turn_removed",
            items: [
              {
                delivery: null,
                id: "agent_removed",
                memoryCitation: null,
                phase: null,
                text: "Removed response",
                type: "agentMessage",
              },
            ],
            itemsView: "summary",
            startedAt: 1,
            status: "completed",
          },
        ],
      }),
    ]);
    expect(cache.snapshot().previewsByThreadId).toEqual({
      thr_1: "**Codex:** Removed response",
    });

    const changed = cache.applyNotification({
      method: "thread/reverted",
      params: { threadId: "thr_1" },
    });

    expect(changed).toBe(true);
    expect(cache.getThread("thr_1")?.turns).toEqual([]);
    expect(cache.snapshot().previewsByThreadId).toEqual({});
  });
});
