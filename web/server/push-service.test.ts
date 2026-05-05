import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";
import * as webPush from "web-push";
import type { PushSubscription, RequestOptions, SendResult } from "web-push";

import type { ServerNotification } from "../src/types/protocol";

import { PushNotificationService } from "./push-service";
import { PushStorage } from "./push-store";

const tempDirs: string[] = [];

async function tempStorage(): Promise<PushStorage> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "codex-web-push-"));
  tempDirs.push(dir);
  return new PushStorage(path.join(dir, "push.json"));
}

function subscription(
  endpoint = "https://push.example/subscription-1",
): PushSubscription {
  return {
    endpoint,
    expirationTime: null,
    keys: {
      auth: "auth",
      p256dh: "p256dh",
    },
  };
}

function turnCompletedNotification(
  status: "completed" | "failed" = "completed",
): ServerNotification {
  return {
    method: "turn/completed",
    params: {
      threadId: "thread-1",
      turn: {
        completedAt: 1,
        durationMs: 1,
        error:
          status === "failed"
            ? {
                additionalDetails: null,
                codexErrorInfo: null,
                message: "The command failed.",
              }
            : null,
        id: "turn-1",
        items: [],
        startedAt: 1,
        status,
      },
    },
  };
}

afterEach(async () => {
  await Promise.all(
    tempDirs
      .splice(0)
      .map((dir) => fs.rm(dir, { force: true, recursive: true })),
  );
});

describe("PushNotificationService", () => {
  it("generates and persists VAPID keys with the runtime web-push module", async () => {
    const store = await tempStorage();
    const service = new PushNotificationService(store);

    const publicKey = await service.getPublicKey();

    expect(publicKey).toMatch(/^[A-Za-z0-9_-]+$/);
    expect((await store.read()).vapidKeys).toEqual(
      expect.objectContaining({
        publicKey,
        privateKey: expect.stringMatching(/^[A-Za-z0-9_-]+$/),
      }),
    );
  });

  it("persists subscriptions and sends classified server notifications", async () => {
    const sent: Array<{
      options: RequestOptions;
      payload: unknown;
      subscription: PushSubscription;
    }> = [];
    const store = await tempStorage();
    const logger = {
      info: vi.fn(),
      warn: vi.fn(),
    };
    const service = new PushNotificationService(
      store,
      "mailto:test@example.com",
      async (targetSubscription, payload, options): Promise<SendResult> => {
        sent.push({
          options,
          payload: JSON.parse(payload) as unknown,
          subscription: targetSubscription,
        });
        return {
          body: "",
          headers: {},
          statusCode: 201,
        };
      },
      () => ({
        privateKey: "private",
        publicKey: "public",
      }),
      logger,
    );

    await service.saveSubscription(subscription(), "test-agent");
    await service.notifyServerNotification(turnCompletedNotification());

    expect(sent).toEqual([
      {
        options: {
          TTL: 3600,
          urgency: "high",
          vapidDetails: {
            privateKey: "private",
            publicKey: "public",
            subject: "mailto:test@example.com",
          },
        },
        payload: {
          body: "Thread is ready.",
          tag: "codex-turn-thread-1-turn-1-completed",
          timestamp: expect.any(Number),
          title: "Codex finished",
          url: "/threads/thread-1",
        },
        subscription: expect.objectContaining(subscription()),
      },
    ]);
    expect(logger.info).toHaveBeenCalledWith("Sending Web Push notification.", {
      subscriptions: 1,
      tag: "codex-turn-thread-1-turn-1-completed",
      title: "Codex finished",
      url: "/threads/thread-1",
    });
    expect(logger.info).toHaveBeenCalledWith(
      "Finished Web Push notification.",
      {
        expired: 0,
        failed: 0,
        sent: 1,
        tag: "codex-turn-thread-1-turn-1-completed",
      },
    );
    expect(await service.getPublicKey()).toBe("public");
  });

  it("dedupes repeated notifications by tag", async () => {
    const sent: string[] = [];
    const logger = {
      info: vi.fn(),
      warn: vi.fn(),
    };
    const service = new PushNotificationService(
      await tempStorage(),
      "mailto:test@example.com",
      async (_targetSubscription, payload): Promise<SendResult> => {
        sent.push(payload);
        return {
          body: "",
          headers: {},
          statusCode: 201,
        };
      },
      () => ({
        privateKey: "private",
        publicKey: "public",
      }),
      logger,
    );

    await service.saveSubscription(subscription(), "test-agent");
    await service.notifyServerNotification(turnCompletedNotification());
    await service.notifyServerNotification(turnCompletedNotification());

    expect(sent).toHaveLength(1);
    expect(logger.info).toHaveBeenCalledWith(
      "Web Push notification skipped: recently sent.",
      {
        tag: "codex-turn-thread-1-turn-1-completed",
        title: "Codex finished",
        url: "/threads/thread-1",
      },
    );
  });

  it("removes push subscriptions that expired upstream", async () => {
    const store = await tempStorage();
    const logger = {
      info: vi.fn(),
      warn: vi.fn(),
    };
    const service = new PushNotificationService(
      store,
      "mailto:test@example.com",
      async (targetSubscription): Promise<SendResult> => {
        throw new webPush.WebPushError(
          "Gone",
          410,
          {},
          "",
          targetSubscription.endpoint,
        );
      },
      () => ({
        privateKey: "private",
        publicKey: "public",
      }),
      logger,
    );

    await service.saveSubscription(subscription(), "test-agent");
    await service.notifyServerNotification(turnCompletedNotification("failed"));

    expect((await store.read()).subscriptions).toEqual([]);
    expect(logger.info).toHaveBeenCalledWith(
      "Finished Web Push notification.",
      {
        expired: 1,
        failed: 0,
        sent: 0,
        tag: "codex-turn-thread-1-turn-1-failed",
      },
    );
  });
});
