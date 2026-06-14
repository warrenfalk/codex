import fs from "node:fs/promises";
import http from "node:http";
import { AddressInfo } from "node:net";
import os from "node:os";
import path from "node:path";

import express from "express";
import { WebSocket, WebSocketServer } from "ws";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD,
  PROXY_THREAD_LIST_UPDATED_METHOD,
} from "../src/lib/proxy-protocol";
import type { JsonRpcRequestMessage } from "../src/lib/jsonrpc";
import type { ServerNotification, Thread } from "../src/types/protocol";

import { attachRelay } from "./relay.js";
import type { PushNotifier, PushNotifyOptions } from "./push-service.js";

type JsonRpcMessage = {
  error?: { code: number; message: string };
  id?: string | number;
  method?: string;
  params?: unknown;
  result?: unknown;
};

async function listen(server: http.Server): Promise<number> {
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });

  return (server.address() as AddressInfo).port;
}

function createCollector(socket: WebSocket) {
  const messages: JsonRpcMessage[] = [];

  socket.on("message", (message) => {
    messages.push(JSON.parse(message.toString()) as JsonRpcMessage);
  });

  return {
    async waitFor(
      predicate: (message: JsonRpcMessage) => boolean,
    ): Promise<JsonRpcMessage> {
      const startedAt = Date.now();
      while (true) {
        const index = messages.findIndex(predicate);
        if (index !== -1) {
          return messages.splice(index, 1)[0]!;
        }
        if (Date.now() - startedAt > 1000) {
          throw new Error(
            `timed out waiting for message; saw ${JSON.stringify(messages)}`,
          );
        }
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
    },
  };
}

async function waitFor<T>(read: () => T | null): Promise<T> {
  const startedAt = Date.now();
  while (true) {
    const value = read();
    if (value) {
      return value;
    }
    if (Date.now() - startedAt > 1000) {
      throw new Error("timed out waiting for value");
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

function buildThread(
  id: string,
  name = id,
  overrides: Partial<Thread> = {},
): Thread {
  return {
    id,
    agentNickname: null,
    agentRole: null,
    cliVersion: "0.0.0",
    createdAt: 1,
    cwd: "/workspace",
    ephemeral: false,
    forkedFromId: null,
    gitInfo: null,
    modelProvider: "openai",
    name,
    path: null,
    preview: `${name} preview`,
    sessionId: id,
    source: "appServer",
    status: { type: "idle" },
    threadSource: null,
    turns: [],
    updatedAt: 1,
    ...overrides,
  };
}

function sendJson(socket: WebSocket, message: JsonRpcMessage): void {
  socket.send(JSON.stringify(message));
}

function installDefaultBackendResponses(
  socket: WebSocket,
  options: { respondToThreadTurnsList?: boolean } = {},
): void {
  const respondToThreadTurnsList = options.respondToThreadTurnsList ?? true;
  socket.on("message", (rawMessage) => {
    const message = JSON.parse(rawMessage.toString()) as JsonRpcMessage;
    if (message.method === "event/firehose") {
      sendJson(socket, {
        id: message.id,
        result: {},
      });
      return;
    }

    if (respondToThreadTurnsList && message.method === "thread/turns/list") {
      sendJson(socket, {
        id: message.id,
        result: {
          backwardsCursor: null,
          data: [],
          nextCursor: null,
        },
      });
    }
  });
}

describe("attachRelay", () => {
  const servers: http.Server[] = [];
  const sockets: WebSocket[] = [];
  const tempDirs: string[] = [];
  const webSocketServers: WebSocketServer[] = [];

  afterEach(async () => {
    for (const socket of sockets) {
      socket.terminate();
    }
    sockets.length = 0;

    await Promise.all(
      webSocketServers.map(
        (server) =>
          new Promise<void>((resolve) => {
            server.clients.forEach((client) => client.terminate());
            server.close(() => resolve());
          }),
      ),
    );
    webSocketServers.length = 0;

    await Promise.all(
      servers.map(
        (server) =>
          new Promise<void>((resolve, reject) => {
            server.close((error) => {
              if (error) {
                reject(error);
                return;
              }
              resolve();
            });
          }),
      ),
    );
    servers.length = 0;

    await Promise.all(
      tempDirs
        .splice(0)
        .map((dir) => fs.rm(dir, { force: true, recursive: true })),
    );
  });

  async function startRelayWithThreads(
    threads: Thread[],
    pushNotifier?: PushNotifier,
  ): Promise<WebSocket> {
    const backendServer = http.createServer();
    servers.push(backendServer);
    const backendWss = new WebSocketServer({ server: backendServer });
    webSocketServers.push(backendWss);
    let backendSocket: WebSocket | null = null;

    backendWss.on("connection", async (socket) => {
      backendSocket = socket;
      sockets.push(socket);
      installDefaultBackendResponses(socket);
      const backendCollector = createCollector(socket);

      const initializeRequest = await backendCollector.waitFor(
        (message) => message.method === "initialize",
      );
      sendJson(socket, {
        id: initializeRequest.id,
        result: {
          codexHome: "/tmp/codex",
          platformFamily: "unix",
          platformOs: "linux",
          userAgent: "codex-test",
        },
      });

      await backendCollector.waitFor(
        (message) => message.method === "initialized",
      );
      await backendCollector.waitFor(
        (message) => message.method === "event/firehose",
      );

      const listRequest = await backendCollector.waitFor(
        (message) => message.method === "thread/list",
      );
      sendJson(socket, {
        id: listRequest.id,
        result: {
          data: threads,
          nextCursor: null,
        },
      });
    });

    const backendPort = await listen(backendServer);

    const relayApp = express();
    const relayServer = http.createServer(relayApp);
    servers.push(relayServer);
    await attachRelay(relayServer, `ws://127.0.0.1:${backendPort}`, {
      pushNotifier,
    });
    await listen(relayServer);

    return await waitFor(() => backendSocket);
  }

  function createPushNotifierCollector() {
    const notifiedRequests: JsonRpcRequestMessage[] = [];
    const notifiedNotifications: ServerNotification[] = [];
    const pushNotifier = {
      notifyServerNotification: vi.fn(
        async (notification: ServerNotification) => {
          notifiedNotifications.push(notification);
        },
      ),
      notifyServerRequest: vi.fn(async (request: JsonRpcRequestMessage) => {
        notifiedRequests.push(request);
      }),
    };

    return { notifiedNotifications, notifiedRequests, pushNotifier };
  }

  it("serves a cached thread list and broadcasts proxy snapshots", async () => {
    const backendServer = http.createServer();
    servers.push(backendServer);
    const backendWss = new WebSocketServer({ server: backendServer });
    webSocketServers.push(backendWss);
    let backendSocket: WebSocket | null = null;

    backendWss.on("connection", async (socket) => {
      backendSocket = socket;
      sockets.push(socket);
      installDefaultBackendResponses(socket);
      const backendCollector = createCollector(socket);

      const initializeRequest = await backendCollector.waitFor(
        (message) => message.method === "initialize",
      );
      expect(initializeRequest.method).toBe("initialize");
      socket.send(
        JSON.stringify({
          id: initializeRequest.id,
          result: {
            codexHome: "/tmp/codex",
            platformFamily: "unix",
            platformOs: "linux",
            userAgent: "codex-test",
          },
        }),
      );

      const initialized = await backendCollector.waitFor(
        (message) => message.method === "initialized",
      );
      expect(initialized.method).toBe("initialized");

      const firehoseRequest = await backendCollector.waitFor(
        (message) => message.method === "event/firehose",
      );
      expect(firehoseRequest.method).toBe("event/firehose");

      const listRequest = await backendCollector.waitFor(
        (message) => message.method === "thread/list",
      );
      expect(listRequest.method).toBe("thread/list");
      socket.send(
        JSON.stringify({
          id: listRequest.id,
          result: {
            data: [buildThread("thread-1", "Thread 1")],
            nextCursor: null,
          },
        }),
      );
    });

    const backendPort = await listen(backendServer);

    const relayApp = express();
    const relayServer = http.createServer(relayApp);
    servers.push(relayServer);
    await attachRelay(relayServer, `ws://127.0.0.1:${backendPort}`);
    const relayPort = await listen(relayServer);

    const client = new WebSocket(`ws://127.0.0.1:${relayPort}/rpc`);
    sockets.push(client);
    const collector = createCollector(client);

    await new Promise<void>((resolve, reject) => {
      client.on("open", () => resolve());
      client.on("error", reject);
    });

    client.send(
      JSON.stringify({
        id: 1,
        method: "initialize",
        params: {
          capabilities: { experimentalApi: true },
          clientInfo: {
            name: "codex_web",
            title: "Codex Web",
            version: "0.0.0",
          },
        },
      }),
    );
    expect(
      await collector.waitFor(
        (message) => message.id === 1 && "result" in message,
      ),
    ).toMatchObject({
      id: 1,
      result: expect.objectContaining({
        platformOs: "linux",
        userAgent: "codex-test",
      }),
    });

    client.send(
      JSON.stringify({
        method: "initialized",
        params: {},
      }),
    );

    expect(
      await collector.waitFor(
        (message) => message.method === PROXY_THREAD_LIST_UPDATED_METHOD,
      ),
    ).toMatchObject({
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: {
        previewsByThreadId: {},
        threadActivityByThreadId: {
          "thread-1": expect.objectContaining({ state: "ready" }),
        },
        threads: [expect.objectContaining({ id: "thread-1" })],
      },
    });

    client.send(
      JSON.stringify({
        id: 2,
        method: "thread/list",
        params: {
          cursor: null,
          limit: 50,
          sortDirection: "desc",
        },
      }),
    );

    expect(
      await collector.waitFor(
        (message) => message.id === 2 && "result" in message,
      ),
    ).toMatchObject({
      id: 2,
      result: {
        data: [expect.objectContaining({ id: "thread-1" })],
        nextCursor: null,
        previewsByThreadId: {},
        threadActivityByThreadId: {
          "thread-1": expect.objectContaining({ state: "ready" }),
        },
      },
    });

    expect(backendSocket).not.toBeNull();
    backendSocket!.send(
      JSON.stringify({
        method: "thread/started",
        params: {
          thread: buildThread("thread-2", "Thread 2"),
        },
      }),
    );

    expect(
      await collector.waitFor(
        (message) =>
          message.method === PROXY_THREAD_LIST_UPDATED_METHOD &&
          Array.isArray((message.params as { threads?: unknown[] }).threads) &&
          (
            (message.params as { threads: Array<{ id: string }> }).threads ?? []
          ).some((thread) => thread.id === "thread-2"),
      ),
    ).toMatchObject({
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: {
        previewsByThreadId: {},
        threadActivityByThreadId: expect.objectContaining({
          "thread-2": expect.objectContaining({ state: "ready" }),
        }),
        threads: [
          expect.objectContaining({ id: "thread-1" }),
          expect.objectContaining({ id: "thread-2" }),
        ],
      },
    });

    backendSocket!.send(
      JSON.stringify({
        method: "turn/started",
        params: {
          threadId: "thread-2",
          turn: {
            completedAt: null,
            durationMs: null,
            error: null,
            id: "turn-1",
            items: [
              {
                content: [
                  {
                    text: "Latest prompt",
                    text_elements: [],
                    type: "text",
                  },
                ],
                id: "item-1",
                type: "userMessage",
              },
            ],
            startedAt: 2,
            status: "inProgress",
          },
        },
      }),
    );

    expect(
      await collector.waitFor(
        (message) =>
          message.method === PROXY_THREAD_LIST_UPDATED_METHOD &&
          (message.params as { previewsByThreadId?: Record<string, string> })
            .previewsByThreadId?.["thread-2"] === "**You:** Latest prompt",
      ),
    ).toMatchObject({
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: {
        threadActivityByThreadId: {
          "thread-1": expect.objectContaining({ state: "ready" }),
          "thread-2": expect.objectContaining({
            lastUserMessage: "Latest prompt",
            state: "working",
          }),
        },
        previewsByThreadId: {
          "thread-2": "**You:** Latest prompt",
        },
      },
    });

    client.close();
  });

  it("hydrates list activity from only the newest turn", async () => {
    const backendServer = http.createServer();
    servers.push(backendServer);
    const backendWss = new WebSocketServer({ server: backendServer });
    webSocketServers.push(backendWss);
    let turnsListParams: unknown = null;

    backendWss.on("connection", async (socket) => {
      sockets.push(socket);
      installDefaultBackendResponses(socket, {
        respondToThreadTurnsList: false,
      });
      const backendCollector = createCollector(socket);

      const initializeRequest = await backendCollector.waitFor(
        (message) => message.method === "initialize",
      );
      sendJson(socket, {
        id: initializeRequest.id,
        result: {
          codexHome: "/tmp/codex",
          platformFamily: "unix",
          platformOs: "linux",
          userAgent: "codex-test",
        },
      });

      await backendCollector.waitFor(
        (message) => message.method === "initialized",
      );
      await backendCollector.waitFor(
        (message) => message.method === "event/firehose",
      );

      const listRequest = await backendCollector.waitFor(
        (message) => message.method === "thread/list",
      );
      sendJson(socket, {
        id: listRequest.id,
        result: {
          data: [buildThread("thread-1", "Thread 1")],
          nextCursor: null,
        },
      });

      const turnsRequest = await backendCollector.waitFor(
        (message) => message.method === "thread/turns/list",
      );
      turnsListParams = turnsRequest.params;
      sendJson(socket, {
        id: turnsRequest.id,
        result: {
          backwardsCursor: null,
          data: [
            {
              completedAt: null,
              durationMs: null,
              error: null,
              id: "turn-1",
              items: [
                {
                  content: [
                    {
                      text: "Most recent prompt",
                      text_elements: [],
                      type: "text",
                    },
                  ],
                  id: "user-1",
                  type: "userMessage",
                },
              ],
              itemsView: "summary",
              startedAt: 5,
              status: "inProgress",
            },
          ],
          nextCursor: "older-turns",
        },
      });
    });

    const backendPort = await listen(backendServer);

    const relayApp = express();
    const relayServer = http.createServer(relayApp);
    servers.push(relayServer);
    await attachRelay(relayServer, `ws://127.0.0.1:${backendPort}`);
    const relayPort = await listen(relayServer);

    const client = new WebSocket(`ws://127.0.0.1:${relayPort}/rpc`);
    sockets.push(client);
    const collector = createCollector(client);

    await new Promise<void>((resolve, reject) => {
      client.on("open", () => resolve());
      client.on("error", reject);
    });

    client.send(
      JSON.stringify({
        id: 1,
        method: "initialize",
        params: {
          capabilities: { experimentalApi: true },
          clientInfo: {
            name: "codex_web",
            title: "Codex Web",
            version: "0.0.0",
          },
        },
      }),
    );
    await collector.waitFor(
      (message) => message.id === 1 && "result" in message,
    );
    client.send(
      JSON.stringify({
        method: "initialized",
        params: {},
      }),
    );

    expect(
      await collector.waitFor(
        (message) =>
          message.method === PROXY_THREAD_LIST_UPDATED_METHOD &&
          (
            message.params as {
              threadActivityByThreadId?: Record<
                string,
                { lastUserMessage?: string }
              >;
            }
          ).threadActivityByThreadId?.["thread-1"]?.lastUserMessage ===
            "Most recent prompt",
      ),
    ).toMatchObject({
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: {
        threadActivityByThreadId: {
          "thread-1": {
            lastAgentMessage: null,
            lastUserMessage: "Most recent prompt",
            state: "working",
          },
        },
      },
    });

    expect(turnsListParams).toMatchObject({
      cursor: null,
      itemsView: "summary",
      limit: 1,
      sortDirection: "desc",
      threadId: "thread-1",
    });

    client.close();
  });

  it("connects to a unix socket backend URL", async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codex-web-"));
    tempDirs.push(tempDir);
    const socketPath = path.join(tempDir, "app-server.sock");
    const backendServer = http.createServer();
    servers.push(backendServer);
    const backendWss = new WebSocketServer({ server: backendServer });
    webSocketServers.push(backendWss);
    const upgradeRequests: Array<{
      extensions: string | undefined;
      host: string | undefined;
      url: string | undefined;
    }> = [];
    backendServer.prependListener("upgrade", (request) => {
      upgradeRequests.push({
        extensions: request.headers["sec-websocket-extensions"],
        host: request.headers.host,
        url: request.url,
      });
    });

    backendWss.on("connection", async (socket) => {
      sockets.push(socket);
      installDefaultBackendResponses(socket);
      const backendCollector = createCollector(socket);

      const initializeRequest = await backendCollector.waitFor(
        (message) => message.method === "initialize",
      );
      socket.send(
        JSON.stringify({
          id: initializeRequest.id,
          result: {
            codexHome: "/tmp/codex",
            platformFamily: "unix",
            platformOs: "linux",
            userAgent: "codex-test",
          },
        }),
      );

      await backendCollector.waitFor(
        (message) => message.method === "initialized",
      );

      await backendCollector.waitFor(
        (message) => message.method === "event/firehose",
      );

      const listRequest = await backendCollector.waitFor(
        (message) => message.method === "thread/list",
      );
      socket.send(
        JSON.stringify({
          id: listRequest.id,
          result: {
            data: [buildThread("thread-uds", "Unix Socket Thread")],
            nextCursor: null,
          },
        }),
      );
    });

    await new Promise<void>((resolve) => {
      backendServer.listen(socketPath, () => resolve());
    });

    const relayApp = express();
    const relayServer = http.createServer(relayApp);
    servers.push(relayServer);
    await attachRelay(relayServer, `unix://${socketPath}`);
    const relayPort = await listen(relayServer);

    const client = new WebSocket(`ws://127.0.0.1:${relayPort}/rpc`);
    sockets.push(client);
    const collector = createCollector(client);

    await new Promise<void>((resolve, reject) => {
      client.on("open", () => resolve());
      client.on("error", reject);
    });

    client.send(
      JSON.stringify({
        id: 1,
        method: "thread/list",
        params: {
          cursor: null,
          limit: 50,
          sortDirection: "desc",
        },
      }),
    );

    expect(
      await collector.waitFor(
        (message) => message.id === 1 && "result" in message,
      ),
    ).toMatchObject({
      id: 1,
      result: {
        data: [expect.objectContaining({ id: "thread-uds" })],
      },
    });
    expect(upgradeRequests).toEqual([
      { extensions: undefined, host: "localhost", url: "/rpc" },
    ]);
  });

  it("notifies push service with foreground thread state by subscription endpoint", async () => {
    const backendServer = http.createServer();
    servers.push(backendServer);
    const backendWss = new WebSocketServer({ server: backendServer });
    webSocketServers.push(backendWss);
    let backendSocket: WebSocket | null = null;

    backendWss.on("connection", async (socket) => {
      backendSocket = socket;
      sockets.push(socket);
      installDefaultBackendResponses(socket);
      const backendCollector = createCollector(socket);

      const initializeRequest = await backendCollector.waitFor(
        (message) => message.method === "initialize",
      );
      socket.send(
        JSON.stringify({
          id: initializeRequest.id,
          result: {
            codexHome: "/tmp/codex",
            platformFamily: "unix",
            platformOs: "linux",
            userAgent: "codex-test",
          },
        }),
      );

      await backendCollector.waitFor(
        (message) => message.method === "initialized",
      );

      await backendCollector.waitFor(
        (message) => message.method === "event/firehose",
      );

      const listRequest = await backendCollector.waitFor(
        (message) => message.method === "thread/list",
      );
      socket.send(
        JSON.stringify({
          id: listRequest.id,
          result: {
            data: [buildThread("thread-1", "Thread 1")],
            nextCursor: null,
          },
        }),
      );
    });

    const backendPort = await listen(backendServer);

    const notifiedRequests: Array<{
      foregroundThreadIdsByEndpoint: Array<[string, string[]]>;
      request: JsonRpcRequestMessage;
    }> = [];
    const notifiedNotifications: Array<{
      foregroundThreadIdsByEndpoint: Array<[string, string[]]>;
      notification: ServerNotification;
      notificationContext: PushNotifyOptions["notificationContext"] | null;
    }> = [];
    const foregroundThreadIdsByEndpoint = (options?: PushNotifyOptions) =>
      [...(options?.foregroundThreadIdsByEndpoint ?? new Map()).entries()].map(
        ([endpoint, threadIds]) =>
          [endpoint, [...threadIds].sort()] as [string, string[]],
      );
    const pushNotifier = {
      notifyServerNotification: vi.fn(
        async (
          notification: ServerNotification,
          options?: PushNotifyOptions,
        ) => {
          notifiedNotifications.push({
            foregroundThreadIdsByEndpoint:
              foregroundThreadIdsByEndpoint(options),
            notification,
            notificationContext: options?.notificationContext ?? null,
          });
        },
      ),
      notifyServerRequest: vi.fn(
        async (request: JsonRpcRequestMessage, options?: PushNotifyOptions) => {
          notifiedRequests.push({
            foregroundThreadIdsByEndpoint:
              foregroundThreadIdsByEndpoint(options),
            request,
          });
        },
      ),
    };
    const relayApp = express();
    const relayServer = http.createServer(relayApp);
    servers.push(relayServer);
    await attachRelay(relayServer, `ws://127.0.0.1:${backendPort}`, {
      pushNotifier,
    });
    const relayPort = await listen(relayServer);

    expect(backendSocket).not.toBeNull();
    backendSocket!.send(
      JSON.stringify({
        id: "request-1",
        method: "item/commandExecution/requestApproval",
        params: {
          approvalId: null,
          command: "echo hi",
          cwd: "/workspace",
          itemId: "item-1",
          parsedCmd: [],
          reason: null,
          threadId: "thread-1",
          turnId: "turn-1",
        },
      }),
    );

    expect(await waitFor(() => notifiedRequests[0] ?? null)).toMatchObject({
      foregroundThreadIdsByEndpoint: [],
      request: {
        id: "request-1",
        method: "item/commandExecution/requestApproval",
        params: {
          threadId: "thread-1",
        },
      },
    });

    const client = new WebSocket(`ws://127.0.0.1:${relayPort}/rpc`);
    sockets.push(client);
    const collector = createCollector(client);

    await new Promise<void>((resolve, reject) => {
      client.on("open", () => resolve());
      client.on("error", reject);
    });

    client.send(
      JSON.stringify({
        id: 1,
        method: "initialize",
        params: {
          capabilities: { experimentalApi: true },
          clientInfo: {
            name: "codex_web",
            title: "Codex Web",
            version: "0.0.0",
          },
        },
      }),
    );
    await collector.waitFor(
      (message) => message.id === 1 && "result" in message,
    );

    client.send(
      JSON.stringify({
        method: "initialized",
        params: {
          foregroundThreadId: "thread-1",
          pushSubscriptionEndpoint: "https://push.example/phone",
        },
      }),
    );

    expect(
      await collector.waitFor((message) => message.id === "request-1"),
    ).toMatchObject({
      id: "request-1",
      method: "item/commandExecution/requestApproval",
    });

    backendSocket!.send(
      JSON.stringify({
        method: "turn/started",
        params: {
          threadId: "thread-1",
          turn: {
            completedAt: null,
            durationMs: null,
            error: null,
            id: "turn-1",
            items: [],
            startedAt: 1,
            status: "inProgress",
          },
        },
      }),
    );
    backendSocket!.send(
      JSON.stringify({
        method: "item/agentMessage/delta",
        params: {
          delta: "Finished checking the notification content.",
          itemId: "agent-1",
          threadId: "thread-1",
          turnId: "turn-1",
        },
      }),
    );
    backendSocket!.send(
      JSON.stringify({
        method: "turn/completed",
        params: {
          threadId: "thread-1",
          turn: {
            completedAt: 2,
            durationMs: 1,
            error: null,
            id: "turn-1",
            items: [],
            startedAt: 1,
            status: "completed",
          },
        },
      }),
    );

    expect(
      await waitFor(
        () =>
          notifiedNotifications.find(
            (entry) => entry.notification.method === "turn/completed",
          ) ?? null,
      ),
    ).toMatchObject({
      foregroundThreadIdsByEndpoint: [
        ["https://push.example/phone", ["thread-1"]],
      ],
      notification: {
        method: "turn/completed",
        params: {
          threadId: "thread-1",
        },
      },
      notificationContext: {
        completedTurnAgentMessage:
          "Finished checking the notification content.",
      },
    });

    const completedNotificationCount = notifiedNotifications.filter(
      (entry) => entry.notification.method === "turn/completed",
    ).length;

    client.send(
      JSON.stringify({
        method: PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD,
        params: {
          foregroundThreadId: null,
          pushSubscriptionEndpoint: "https://push.example/phone-updated",
        },
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));

    backendSocket!.send(
      JSON.stringify({
        method: "turn/completed",
        params: {
          threadId: "thread-1",
          turn: {
            completedAt: 3,
            durationMs: 1,
            error: null,
            id: "turn-2",
            items: [],
            startedAt: 2,
            status: "completed",
          },
        },
      }),
    );

    expect(
      await waitFor(() => {
        const completedNotifications = notifiedNotifications.filter(
          (entry) => entry.notification.method === "turn/completed",
        );
        return completedNotifications.length > completedNotificationCount
          ? completedNotifications.at(-1)!
          : null;
      }),
    ).toMatchObject({
      foregroundThreadIdsByEndpoint: [],
      notification: {
        method: "turn/completed",
        params: {
          threadId: "thread-1",
        },
      },
      notificationContext: {
        completedTurnAgentMessage: null,
      },
    });
  });

  it("does not notify push service for hidden subagent or unknown server requests", async () => {
    const { notifiedRequests, pushNotifier } = createPushNotifierCollector();
    const backendSocket = await startRelayWithThreads(
      [
        buildThread("thread-1", "Thread 1"),
        buildThread("subagent-thread", "Subagent Thread", {
          source: { subAgent: "review" },
        }),
      ],
      pushNotifier,
    );

    backendSocket.send(
      JSON.stringify({
        id: "subagent-request",
        method: "item/commandExecution/requestApproval",
        params: {
          approvalId: null,
          command: "echo hidden",
          cwd: "/workspace",
          itemId: "item-1",
          parsedCmd: [],
          reason: null,
          threadId: "subagent-thread",
          turnId: "turn-1",
        },
      }),
    );
    backendSocket.send(
      JSON.stringify({
        id: "unknown-request",
        method: "item/commandExecution/requestApproval",
        params: {
          approvalId: null,
          command: "echo unknown",
          cwd: "/workspace",
          itemId: "item-1",
          parsedCmd: [],
          reason: null,
          threadId: "unknown-thread",
          turnId: "turn-1",
        },
      }),
    );
    backendSocket.send(
      JSON.stringify({
        id: "visible-request",
        method: "item/commandExecution/requestApproval",
        params: {
          approvalId: null,
          command: "echo visible",
          cwd: "/workspace",
          itemId: "item-1",
          parsedCmd: [],
          reason: null,
          threadId: "thread-1",
          turnId: "turn-1",
        },
      }),
    );

    expect(
      await waitFor(
        () =>
          notifiedRequests.find(
            (request) => request.id === "visible-request",
          ) ?? null,
      ),
    ).toMatchObject({
      id: "visible-request",
      params: {
        threadId: "thread-1",
      },
    });
    expect(pushNotifier.notifyServerRequest).toHaveBeenCalledTimes(1);
    expect(notifiedRequests.map((request) => request.id)).toEqual([
      "visible-request",
    ]);
  });

  it("does not notify push service for hidden subagent or unknown turn completions", async () => {
    const { notifiedNotifications, pushNotifier } =
      createPushNotifierCollector();
    const backendSocket = await startRelayWithThreads(
      [
        buildThread("thread-1", "Thread 1"),
        buildThread("subagent-thread", "Subagent Thread", {
          threadSource: "subagent",
        }),
      ],
      pushNotifier,
    );

    const completedTurn = (id: string) => ({
      completedAt: 2,
      durationMs: 1,
      error: null,
      id,
      items: [],
      startedAt: 1,
      status: "completed",
    });

    backendSocket.send(
      JSON.stringify({
        method: "turn/completed",
        params: {
          threadId: "subagent-thread",
          turn: completedTurn("subagent-turn"),
        },
      }),
    );
    backendSocket.send(
      JSON.stringify({
        method: "turn/completed",
        params: {
          threadId: "unknown-thread",
          turn: completedTurn("unknown-turn"),
        },
      }),
    );
    backendSocket.send(
      JSON.stringify({
        method: "turn/completed",
        params: {
          threadId: "thread-1",
          turn: completedTurn("visible-turn"),
        },
      }),
    );

    expect(
      await waitFor(
        () =>
          notifiedNotifications.find(
            (notification) =>
              notification.method === "turn/completed" &&
              notification.params.turn.id === "visible-turn",
          ) ?? null,
      ),
    ).toMatchObject({
      method: "turn/completed",
      params: {
        threadId: "thread-1",
      },
    });
    expect(pushNotifier.notifyServerNotification).toHaveBeenCalledTimes(1);
    expect(
      notifiedNotifications.map((notification) =>
        notification.method === "turn/completed"
          ? notification.params.turn.id
          : notification.method,
      ),
    ).toEqual(["visible-turn"]);
  });
});
