import { describe, expect, it } from "vitest";

import type { ServerRequest } from "../../../codex-rs/app-server-protocol/schema/typescript/ServerRequest";

import {
  type AppState,
  appReducer,
  initialState,
  setServerNotification,
  setServerRequest,
} from "./thread-state";

describe("thread state reducer", () => {
  it("adds pending server requests and resolves them", () => {
    const withRequest = appReducer(
      initialState,
      setServerRequest({
        method: "item/fileChange/requestApproval",
        id: 42,
        params: {
          threadId: "thr_1",
          turnId: "turn_1",
          itemId: "item_1",
          startedAtMs: 1,
          reason: "Need approval",
          grantRoot: null,
        },
      } satisfies ServerRequest),
    );

    expect(withRequest.pendingRequests).toHaveLength(1);

    const resolved = appReducer(
      withRequest,
      setServerNotification({
        method: "serverRequest/resolved",
        params: {
          threadId: "thr_1",
          requestId: 42,
        },
      }),
    );

    expect(resolved.pendingRequests).toHaveLength(0);
  });

  it("applies streaming agent deltas", () => {
    const threadState: AppState = {
      ...initialState,
      threads: [
        {
          id: "thr_1",
          sessionId: "thr_1",
          forkedFromId: null,
          preview: "preview",
          ephemeral: false,
          modelProvider: "openai",
          createdAt: 1,
          updatedAt: 1,
          status: { type: "idle" },
          path: null,
          cwd: "/tmp/project",
          cliVersion: "0.0.0",
          source: "appServer",
          threadSource: null,
          agentNickname: null,
          agentRole: null,
          gitInfo: null,
          name: "Thread",
          turns: [
            {
              id: "turn_1",
              items: [
                {
                  type: "agentMessage",
                  id: "item_1",
                  text: "Hello",
                  phase: null,
                  memoryCitation: null,
                },
              ],
              itemsView: "full",
              status: "inProgress",
              error: null,
              startedAt: 1,
              completedAt: null,
              durationMs: null,
            },
          ],
        },
      ],
    };

    const next = appReducer(
      threadState,
      setServerNotification({
        method: "item/agentMessage/delta",
        params: {
          threadId: "thr_1",
          turnId: "turn_1",
          itemId: "item_1",
          delta: " world",
        },
      }),
    );

    const agentItem = next.threads[0]?.turns[0]?.items[0];
    expect(agentItem).toMatchObject({
      type: "agentMessage",
      text: "Hello world",
    });
  });

  it("applies file change patch updates", () => {
    const threadState: AppState = {
      ...initialState,
      threads: [
        {
          id: "thr_1",
          sessionId: "thr_1",
          forkedFromId: null,
          preview: "preview",
          ephemeral: false,
          modelProvider: "openai",
          createdAt: 1,
          updatedAt: 1,
          status: { type: "active", activeFlags: [] },
          path: null,
          cwd: "/tmp/project",
          cliVersion: "0.0.0",
          source: "appServer",
          threadSource: null,
          agentNickname: null,
          agentRole: null,
          gitInfo: null,
          name: "Thread",
          turns: [
            {
              id: "turn_1",
              items: [],
              itemsView: "full",
              status: "inProgress",
              error: null,
              startedAt: 1,
              completedAt: null,
              durationMs: null,
            },
          ],
        },
      ],
    };

    const next = appReducer(
      threadState,
      setServerNotification({
        method: "item/fileChange/patchUpdated",
        params: {
          threadId: "thr_1",
          turnId: "turn_1",
          itemId: "patch_1",
          changes: [
            {
              path: "src/lib/thread-state.ts",
              kind: { type: "update", move_path: null },
              diff: "@@ -1 +1 @@",
            },
          ],
        },
      }),
    );

    expect(next.threads[0]?.turns[0]?.items).toEqual([
      {
        type: "fileChange",
        id: "patch_1",
        status: "inProgress",
        changes: [
          {
            path: "src/lib/thread-state.ts",
            kind: { type: "update", move_path: null },
            diff: "@@ -1 +1 @@",
          },
        ],
      },
    ]);
  });
});
