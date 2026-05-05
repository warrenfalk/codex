import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { PushStorage } from "./push-store";

const tempDirs: string[] = [];

async function tempStorage(): Promise<PushStorage> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "codex-web-push-"));
  tempDirs.push(dir);
  return new PushStorage(path.join(dir, "push.json"));
}

afterEach(async () => {
  await Promise.all(
    tempDirs
      .splice(0)
      .map((dir) => fs.rm(dir, { force: true, recursive: true })),
  );
});

describe("PushStorage", () => {
  it("returns empty data before the file exists", async () => {
    const store = await tempStorage();

    expect(await store.read()).toEqual({
      subscriptions: [],
      vapidKeys: null,
      version: 1,
    });
  });

  it("persists VAPID keys and subscriptions", async () => {
    const store = await tempStorage();

    await store.update((data) => ({
      data: {
        ...data,
        subscriptions: [
          {
            createdAt: "2026-05-05T00:00:00.000Z",
            endpoint: "https://push.example/subscription-1",
            expirationTime: null,
            keys: {
              auth: "auth",
              p256dh: "p256dh",
            },
            updatedAt: "2026-05-05T00:00:00.000Z",
            userAgent: "test-agent",
          },
        ],
        vapidKeys: {
          privateKey: "private",
          publicKey: "public",
        },
      },
      result: undefined,
    }));

    expect(await store.read()).toEqual({
      subscriptions: [
        {
          createdAt: "2026-05-05T00:00:00.000Z",
          endpoint: "https://push.example/subscription-1",
          expirationTime: null,
          keys: {
            auth: "auth",
            p256dh: "p256dh",
          },
          updatedAt: "2026-05-05T00:00:00.000Z",
          userAgent: "test-agent",
        },
      ],
      vapidKeys: {
        privateKey: "private",
        publicKey: "public",
      },
      version: 1,
    });
  });
});
