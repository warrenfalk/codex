import http from "node:http";
import { AddressInfo } from "node:net";

import express from "express";
import { WebSocket, WebSocketServer } from "ws";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PROXY_THREAD_LIST_UPDATED_METHOD } from "../src/lib/proxy-protocol";
import type { JsonRpcRequestMessage } from "../src/lib/jsonrpc";
import type { Thread } from "../src/types/protocol";

import { attachRelay } from "./relay.js";

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

function buildThread(id: string, name = id): Thread {
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
    source: "appServer",
    status: { type: "idle" },
    turns: [],
    updatedAt: 1,
  };
}

describe("attachRelay", () => {
  const servers: http.Server[] = [];
  const sockets: WebSocket[] = [];
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
  });

  it("serves a cached thread list and broadcasts proxy snapshots", async () => {
    const backendServer = http.createServer();
    servers.push(backendServer);
    const backendWss = new WebSocketServer({ server: backendServer });
    webSocketServers.push(backendWss);
    let backendSocket: WebSocket | null = null;

    backendWss.on("connection", async (socket) => {
      backendSocket = socket;
      sockets.push(socket);
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
        previewsByThreadId: {
          "thread-2": "**You:** Latest prompt",
        },
      },
    });

    client.close();
  });

  it("notifies the push service for server requests when no browser is connected", async () => {
    const backendServer = http.createServer();
    servers.push(backendServer);
    const backendWss = new WebSocketServer({ server: backendServer });
    webSocketServers.push(backendWss);
    let backendSocket: WebSocket | null = null;

    backendWss.on("connection", async (socket) => {
      backendSocket = socket;
      sockets.push(socket);
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

    const notifiedRequests: JsonRpcRequestMessage[] = [];
    const pushNotifier = {
      notifyServerNotification: vi.fn(async () => undefined),
      notifyServerRequest: vi.fn(async (request: JsonRpcRequestMessage) => {
        notifiedRequests.push(request);
      }),
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
      id: "request-1",
      method: "item/commandExecution/requestApproval",
      params: {
        threadId: "thread-1",
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
        params: {},
      }),
    );

    expect(
      await collector.waitFor((message) => message.id === "request-1"),
    ).toMatchObject({
      id: "request-1",
      method: "item/commandExecution/requestApproval",
    });
  });
});
