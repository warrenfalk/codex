import fs from "node:fs/promises";
import http from "node:http";
import type { AddressInfo } from "node:net";
import os from "node:os";
import path from "node:path";

import express from "express";
import { afterEach, describe, expect, it } from "vitest";

import { SOURCE_FILE_READ_PATH } from "../src/lib/source-file-protocol";
import type { Thread } from "../src/types/protocol";

import { createSourceFileRouter } from "./source-files.js";

const servers: http.Server[] = [];
const tempDirs: string[] = [];

async function listen(server: http.Server): Promise<number> {
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  return (server.address() as AddressInfo).port;
}

async function closeServer(server: http.Server): Promise<void> {
  await new Promise<void>((resolve) => {
    server.close(() => resolve());
  });
}

async function tempDir(): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "codex-web-files-"));
  tempDirs.push(dir);
  return dir;
}

function buildThread(cwd: string): Thread {
  return {
    id: "thread-1",
    agentNickname: null,
    agentRole: null,
    cliVersion: "0.0.0",
    createdAt: 1,
    cwd,
    ephemeral: false,
    forkedFromId: null,
    gitInfo: null,
    modelProvider: "openai",
    name: "Thread 1",
    parentThreadId: null,
    path: null,
    preview: "preview",
    sessionId: "thread-1",
    source: "appServer",
    status: { type: "idle" },
    threadSource: null,
    turns: [],
    updatedAt: 1,
  };
}

async function startSourceFileServer(thread: Thread): Promise<string> {
  const app = express();
  app.use(
    createSourceFileRouter({
      getThread: (threadId) => (threadId === thread.id ? thread : null),
    }),
  );
  const server = http.createServer(app);
  servers.push(server);
  const port = await listen(server);
  return `http://127.0.0.1:${port}`;
}

afterEach(async () => {
  await Promise.all(servers.splice(0).map(closeServer));
  await Promise.all(
    tempDirs
      .splice(0)
      .map((dir) => fs.rm(dir, { force: true, recursive: true })),
  );
});

describe("source file router", () => {
  it("reads UTF-8 source files under the thread cwd", async () => {
    const root = await tempDir();
    await fs.mkdir(path.join(root, "src"));
    await fs.writeFile(path.join(root, "src", "app.ts"), "const value = 1;\n");
    const baseUrl = await startSourceFileServer(buildThread(root));
    const params = new URLSearchParams({
      path: "src/app.ts:1",
      threadId: "thread-1",
    });

    const response = await fetch(
      `${baseUrl}${SOURCE_FILE_READ_PATH}?${params}`,
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({
      content: "const value = 1;\n",
      displayPath: "src/app.ts",
      fileKind: "source",
      language: "ts",
      line: 1,
      path: path.join(root, "src", "app.ts"),
      root,
      sizeBytes: 17,
    });
  });

  it("rejects paths outside the thread cwd", async () => {
    const root = await tempDir();
    const outside = await tempDir();
    await fs.writeFile(path.join(outside, "secret.txt"), "secret\n");
    const baseUrl = await startSourceFileServer(buildThread(root));
    const params = new URLSearchParams({
      path: path.join(outside, "secret.txt"),
      threadId: "thread-1",
    });

    const response = await fetch(
      `${baseUrl}${SOURCE_FILE_READ_PATH}?${params}`,
    );

    expect(response.status).toBe(403);
    expect(await response.json()).toEqual({
      error: "File path is outside the thread root.",
    });
  });

  it("rejects known image previews for v1", async () => {
    const root = await tempDir();
    await fs.writeFile(path.join(root, "diagram.png"), "not really a png");
    const baseUrl = await startSourceFileServer(buildThread(root));
    const params = new URLSearchParams({
      path: "diagram.png",
      threadId: "thread-1",
    });

    const response = await fetch(
      `${baseUrl}${SOURCE_FILE_READ_PATH}?${params}`,
    );

    expect(response.status).toBe(415);
    expect(await response.json()).toEqual({
      error: "File preview is not supported.",
      fileKind: "image",
      language: "png",
    });
  });
});
