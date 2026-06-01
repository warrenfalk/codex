import fs from "node:fs/promises";
import http from "node:http";
import type { AddressInfo } from "node:net";
import os from "node:os";
import path from "node:path";

import { WebSocket, WebSocketServer } from "ws";
import { afterEach, describe, expect, it } from "vitest";

import { startServer, type RunningWebServer } from "./index.js";

type JsonRpcMessage = {
  id?: string | number;
  method?: string;
  result?: unknown;
};

const servers: http.Server[] = [];
const sockets: WebSocket[] = [];
const tempDirs: string[] = [];
const webSocketServers: WebSocketServer[] = [];

async function listen(server: http.Server): Promise<number> {
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  return (server.address() as AddressInfo).port;
}

async function unusedPort(): Promise<number> {
  const server = http.createServer();
  const port = await listen(server);
  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
  return port;
}

function sendJson(socket: WebSocket, message: JsonRpcMessage): void {
  socket.send(JSON.stringify(message));
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

async function createBackend(): Promise<string> {
  const backendServer = http.createServer();
  servers.push(backendServer);
  const backendWss = new WebSocketServer({ server: backendServer });
  webSocketServers.push(backendWss);

  backendWss.on("connection", async (socket) => {
    sockets.push(socket);
    const collector = createCollector(socket);

    const initializeRequest = await collector.waitFor(
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

    await collector.waitFor((message) => message.method === "initialized");

    const firehoseRequest = await collector.waitFor(
      (message) => message.method === "event/firehose",
    );
    sendJson(socket, {
      id: firehoseRequest.id,
      result: {},
    });

    const listRequest = await collector.waitFor(
      (message) => message.method === "thread/list",
    );
    sendJson(socket, {
      id: listRequest.id,
      result: {
        data: [],
        nextCursor: null,
      },
    });
  });

  const backendPort = await listen(backendServer);
  return `ws://127.0.0.1:${backendPort}`;
}

async function tempDir(): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "codex-web-server-"));
  tempDirs.push(dir);
  return dir;
}

async function closeServer(server: http.Server): Promise<void> {
  await new Promise<void>((resolve) => {
    server.close(() => resolve());
  });
}

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

  await Promise.all(servers.splice(0).map(closeServer));

  await Promise.all(
    tempDirs
      .splice(0)
      .map((dir) => fs.rm(dir, { force: true, recursive: true })),
  );
});

describe("startServer", () => {
  it("writes the control file after listen and removes it on shutdown", async () => {
    const backendUrl = await createBackend();
    const dir = await tempDir();
    const controlPath = path.join(dir, "control.json");
    const port = await unusedPort();
    let running: RunningWebServer | null = null;

    try {
      running = await startServer({
        CODEX_WEB_BACKEND_URL: backendUrl,
        CODEX_WEB_CONTROL_PATH: controlPath,
        CODEX_WEB_HOST: "0.0.0.0",
        CODEX_WEB_PROXY_PORT: String(port),
      });

      const control = JSON.parse(
        await fs.readFile(controlPath, "utf8"),
      ) as unknown;
      expect(control).toEqual({
        baseUrl: `http://127.0.0.1:${port}`,
        pid: process.pid,
        shutdownUrl: `http://127.0.0.1:${port}/api/control/shutdown`,
        startedAt: expect.any(String),
        token: running.control.token,
        version: 1,
      });
      expect((await fs.stat(controlPath)).mode & 0o777).toBe(0o600);

      const client = new WebSocket(`ws://127.0.0.1:${port}/rpc`);
      sockets.push(client);
      const clientClosed = new Promise<number>((resolve) => {
        client.on("close", (code) => resolve(code));
      });
      await new Promise<void>((resolve, reject) => {
        client.on("open", () => resolve());
        client.on("error", reject);
      });

      const response = await fetch(running.control.shutdownUrl, {
        headers: {
          authorization: `Bearer ${running.control.token}`,
        },
        method: "POST",
      });

      expect(response.status).toBe(200);
      expect(await response.json()).toEqual({
        ok: true,
        shuttingDown: true,
      });
      expect(await clientClosed).toBe(1001);
      await running.shutdown();
      await expect(fs.access(controlPath)).rejects.toMatchObject({
        code: "ENOENT",
      });
    } finally {
      await running?.shutdown();
    }
  });
});
