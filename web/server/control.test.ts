import fs from "node:fs/promises";
import http from "node:http";
import type { AddressInfo } from "node:net";
import os from "node:os";
import path from "node:path";

import express from "express";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  CONTROL_SHUTDOWN_PATH,
  buildControlFileData,
  createControlRouter,
  readControlFile,
  resolveControlFilePath,
  writeControlFile,
} from "./control.js";

const tempDirs: string[] = [];

async function tempDir(): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "codex-web-control-"));
  tempDirs.push(dir);
  return dir;
}

async function listen(app: express.Express): Promise<{
  baseUrl: string;
  server: http.Server;
}> {
  const server = http.createServer(app);
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const port = (server.address() as AddressInfo).port;
  return {
    baseUrl: `http://127.0.0.1:${port}`,
    server,
  };
}

async function close(server: http.Server): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

async function waitFor(read: () => boolean): Promise<void> {
  const startedAt = Date.now();
  while (!read()) {
    if (Date.now() - startedAt > 1000) {
      throw new Error("timed out waiting for condition");
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

afterEach(async () => {
  await Promise.all(
    tempDirs
      .splice(0)
      .map((dir) => fs.rm(dir, { force: true, recursive: true })),
  );
});

describe("control file paths", () => {
  it("defaults to CODEX_HOME", () => {
    expect(resolveControlFilePath({ CODEX_HOME: "/tmp/codex-home" })).toBe(
      "/tmp/codex-home/codex-web-control.json",
    );
  });

  it("falls back to the default codex home", () => {
    expect(resolveControlFilePath({})).toBe(
      path.join(os.homedir(), ".codex", "codex-web-control.json"),
    );
  });

  it("honors CODEX_WEB_CONTROL_PATH", () => {
    expect(
      resolveControlFilePath({
        CODEX_HOME: "/tmp/codex-home",
        CODEX_WEB_CONTROL_PATH: "relative-control.json",
      }),
    ).toBe(path.resolve("relative-control.json"));
  });
});

describe("control file storage", () => {
  it("writes a readable 0600 control file", async () => {
    const dir = await tempDir();
    const filePath = path.join(dir, "control.json");
    const data = buildControlFileData(
      "0.0.0.0",
      4200,
      "token",
      new Date("2026-05-31T12:00:00.000Z"),
    );

    await writeControlFile(filePath, data);

    expect(await readControlFile(filePath)).toEqual({
      ...data,
      baseUrl: "http://127.0.0.1:4200",
      shutdownUrl: "http://127.0.0.1:4200/api/control/shutdown",
    });
    expect((await fs.stat(filePath)).mode & 0o777).toBe(0o600);
  });
});

describe("createControlRouter", () => {
  it("rejects missing bearer tokens", async () => {
    const onShutdown = vi.fn();
    const app = express();
    app.use(createControlRouter({ onShutdown, token: "secret" }));
    const { baseUrl, server } = await listen(app);

    try {
      const response = await fetch(`${baseUrl}${CONTROL_SHUTDOWN_PATH}`, {
        method: "POST",
      });

      expect(response.status).toBe(401);
      expect(await response.json()).toEqual({
        error: "missing bearer token",
        ok: false,
      });
      expect(onShutdown).not.toHaveBeenCalled();
    } finally {
      await close(server);
    }
  });

  it("rejects wrong bearer tokens", async () => {
    const onShutdown = vi.fn();
    const app = express();
    app.use(createControlRouter({ onShutdown, token: "secret" }));
    const { baseUrl, server } = await listen(app);

    try {
      const response = await fetch(`${baseUrl}${CONTROL_SHUTDOWN_PATH}`, {
        headers: {
          authorization: "Bearer wrong",
        },
        method: "POST",
      });

      expect(response.status).toBe(403);
      expect(await response.json()).toEqual({
        error: "invalid bearer token",
        ok: false,
      });
      expect(onShutdown).not.toHaveBeenCalled();
    } finally {
      await close(server);
    }
  });

  it("accepts the bearer token and triggers shutdown once", async () => {
    const onShutdown = vi.fn();
    const app = express();
    app.use(createControlRouter({ onShutdown, token: "secret" }));
    const { baseUrl, server } = await listen(app);

    try {
      const request = () =>
        fetch(`${baseUrl}${CONTROL_SHUTDOWN_PATH}`, {
          headers: {
            authorization: "Bearer secret",
          },
          method: "POST",
        });

      await expect((await request()).json()).resolves.toEqual({
        ok: true,
        shuttingDown: true,
      });
      await expect((await request()).json()).resolves.toEqual({
        ok: true,
        shuttingDown: true,
      });
      await waitFor(() => onShutdown.mock.calls.length === 1);
    } finally {
      await close(server);
    }
  });
});
