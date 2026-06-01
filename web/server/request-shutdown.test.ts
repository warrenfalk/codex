import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  CONTROL_FILE_VERSION,
  buildControlFileData,
  writeControlFile,
} from "./control.js";
import { requestShutdown } from "./request-shutdown.js";

const tempDirs: string[] = [];

async function tempControlPath(): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "codex-web-shutdown-"));
  tempDirs.push(dir);
  return path.join(dir, "control.json");
}

afterEach(async () => {
  await Promise.all(
    tempDirs
      .splice(0)
      .map((dir) => fs.rm(dir, { force: true, recursive: true })),
  );
});

describe("requestShutdown", () => {
  it("fails clearly when the control file is missing", async () => {
    const controlPath = await tempControlPath();
    const fetchImpl = vi.fn();

    await expect(
      requestShutdown({ CODEX_WEB_CONTROL_PATH: controlPath }, fetchImpl),
    ).rejects.toThrow(`No codex web control file found at ${controlPath}`);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("fails clearly when the control file is stale", async () => {
    const controlPath = await tempControlPath();
    await fs.writeFile(
      controlPath,
      `${JSON.stringify({
        baseUrl: "http://127.0.0.1:4200",
        pid: 2_147_483_647,
        shutdownUrl: "http://127.0.0.1:4200/api/control/shutdown",
        startedAt: "2026-05-31T12:00:00.000Z",
        token: "secret",
        version: CONTROL_FILE_VERSION,
      })}\n`,
    );
    const fetchImpl = vi.fn();

    await expect(
      requestShutdown({ CODEX_WEB_CONTROL_PATH: controlPath }, fetchImpl),
    ).rejects.toThrow(
      `Stale codex web control file at ${controlPath}: process 2147483647 is not running`,
    );
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("sends the expected bearer token to the shutdown URL", async () => {
    const controlPath = await tempControlPath();
    const control = buildControlFileData(
      "127.0.0.1",
      4200,
      "secret",
      new Date("2026-05-31T12:00:00.000Z"),
    );
    await writeControlFile(controlPath, control);
    const fetchImpl = vi.fn(async () => new Response("{}", { status: 200 }));

    await requestShutdown({ CODEX_WEB_CONTROL_PATH: controlPath }, fetchImpl);

    expect(fetchImpl).toHaveBeenCalledWith(control.shutdownUrl, {
      headers: {
        authorization: "Bearer secret",
      },
      method: "POST",
    });
  });

  it("fails clearly when the shutdown request is rejected", async () => {
    const controlPath = await tempControlPath();
    await writeControlFile(
      controlPath,
      buildControlFileData(
        "127.0.0.1",
        4200,
        "secret",
        new Date("2026-05-31T12:00:00.000Z"),
      ),
    );
    const fetchImpl = vi.fn(
      async () => new Response("forbidden", { status: 403 }),
    );

    await expect(
      requestShutdown({ CODEX_WEB_CONTROL_PATH: controlPath }, fetchImpl),
    ).rejects.toThrow("Shutdown request failed with HTTP 403: forbidden");
  });
});
