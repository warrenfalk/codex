import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import type { Request } from "express";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as webPush from "web-push";
import type { PushSubscription, RequestOptions, SendResult } from "web-push";

import type { ServerNotification } from "../src/types/protocol";

import {
  PushNotificationService,
  vapidSubjectFromRequest,
} from "./push-service";
import { PushStorage } from "./push-store";

const tempDirs: string[] = [];

function requestWithHeaders(
  headers: Record<string, string>,
): Pick<Request, "get"> {
  return {
    get(name: string): string | undefined {
      return headers[name.toLowerCase()];
    },
  } as Pick<Request, "get">;
}

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
        itemsView: "full",
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
  it("uses a public HTTPS request origin as a VAPID subject hint", () => {
    expect(
      vapidSubjectFromRequest(
        requestWithHeaders({
          origin: "https://agent.example/threads/thread-1",
        }),
      ),
    ).toBe("https://agent.example");
    expect(
      vapidSubjectFromRequest(
        requestWithHeaders({
          "x-forwarded-host": "agent.example",
          "x-forwarded-proto": "https",
        }),
      ),
    ).toBe("https://agent.example");
    expect(
      vapidSubjectFromRequest(
        requestWithHeaders({
          origin: "http://localhost:4200",
        }),
      ),
    ).toBeNull();
  });

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

    await service.saveSubscription(subscription(), "test-agent", null);
    await service.notifyServerNotification(turnCompletedNotification(), {
      notificationContext: {
        completedTurnAgentMessage: "The agent finished the requested work.",
      },
    });

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
          body: "The agent finished the requested work.",
          tag: "codex-turn-thread-1-turn-1-completed",
          threadId: "thread-1",
          timestamp: expect.any(Number),
          title: "Codex finished",
          url: "/threads/thread-1",
        },
        subscription: expect.objectContaining(subscription()),
      },
    ]);
    expect(logger.info).toHaveBeenCalledWith("Sending Web Push notification.", {
      foregroundSubscriptions: 0,
      storedSubscriptions: 1,
      subscriptions: 1,
      tag: "codex-turn-thread-1-turn-1-completed",
      threadId: "thread-1",
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

  it("uses the saved public origin as the VAPID subject when no subject is configured", async () => {
    const sent: RequestOptions[] = [];
    const store = await tempStorage();
    const service = new PushNotificationService(
      store,
      null,
      async (_targetSubscription, _payload, options): Promise<SendResult> => {
        sent.push(options);
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
      {
        info: vi.fn(),
        warn: vi.fn(),
      },
    );

    await service.saveSubscription(
      subscription(),
      "test-agent",
      "https://agent.example",
    );
    await service.notifyServerNotification(turnCompletedNotification());

    expect(sent).toEqual([
      expect.objectContaining({
        vapidDetails: expect.objectContaining({
          subject: "https://agent.example",
        }),
      }),
    ]);
    expect((await store.read()).vapidSubject).toBe("https://agent.example");
  });

  it("skips sends until a VAPID subject is configured or saved", async () => {
    const sent: RequestOptions[] = [];
    const logger = {
      info: vi.fn(),
      warn: vi.fn(),
    };
    const service = new PushNotificationService(
      await tempStorage(),
      null,
      async (_targetSubscription, _payload, options): Promise<SendResult> => {
        sent.push(options);
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

    await service.saveSubscription(subscription(), "test-agent", null);
    await service.notifyServerNotification(turnCompletedNotification());

    expect(sent).toEqual([]);
    expect(logger.warn).toHaveBeenCalledWith(
      "Web Push notification skipped: missing VAPID subject. Set CODEX_WEB_PUSH_VAPID_SUBJECT or re-save the subscription from the public HTTPS origin.",
      {
        tag: "codex-turn-thread-1-turn-1-completed",
        threadId: "thread-1",
        title: "Codex finished",
        url: "/threads/thread-1",
      },
    );
  });

  it("sends only to subscriptions without the notified thread in the foreground", async () => {
    const sent: PushSubscription[] = [];
    const store = await tempStorage();
    const logger = {
      info: vi.fn(),
      warn: vi.fn(),
    };
    const foregroundSubscription = subscription(
      "https://push.example/foreground",
    );
    const backgroundSubscription = subscription(
      "https://push.example/background",
    );
    const service = new PushNotificationService(
      store,
      "mailto:test@example.com",
      async (targetSubscription): Promise<SendResult> => {
        sent.push(targetSubscription);
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

    await service.saveSubscription(
      foregroundSubscription,
      "foreground-agent",
      null,
    );
    await service.saveSubscription(
      backgroundSubscription,
      "background-agent",
      null,
    );
    await service.notifyServerNotification(turnCompletedNotification(), {
      foregroundThreadIdsByEndpoint: new Map([
        [foregroundSubscription.endpoint, new Set(["thread-1"])],
        [backgroundSubscription.endpoint, new Set(["thread-2"])],
      ]),
    });

    expect(sent).toEqual([expect.objectContaining(backgroundSubscription)]);
    expect(logger.info).toHaveBeenCalledWith("Sending Web Push notification.", {
      foregroundSubscriptions: 1,
      storedSubscriptions: 2,
      subscriptions: 1,
      tag: "codex-turn-thread-1-turn-1-completed",
      threadId: "thread-1",
      title: "Codex finished",
      url: "/threads/thread-1",
    });
  });

  it("skips when every subscribed client has the notified thread in the foreground", async () => {
    const sent: PushSubscription[] = [];
    const store = await tempStorage();
    const logger = {
      info: vi.fn(),
      warn: vi.fn(),
    };
    const foregroundSubscription = subscription(
      "https://push.example/foreground",
    );
    const service = new PushNotificationService(
      store,
      "mailto:test@example.com",
      async (targetSubscription): Promise<SendResult> => {
        sent.push(targetSubscription);
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

    await service.saveSubscription(
      foregroundSubscription,
      "foreground-agent",
      null,
    );
    await service.notifyServerNotification(turnCompletedNotification(), {
      foregroundThreadIdsByEndpoint: new Map([
        [foregroundSubscription.endpoint, new Set(["thread-1"])],
      ]),
    });

    expect(sent).toEqual([]);
    expect(logger.info).toHaveBeenCalledWith(
      "Web Push notification skipped: thread is foreground on all subscribed clients.",
      {
        foregroundSubscriptions: 1,
        storedSubscriptions: 1,
        tag: "codex-turn-thread-1-turn-1-completed",
        threadId: "thread-1",
        title: "Codex finished",
        url: "/threads/thread-1",
      },
    );
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

    await service.saveSubscription(subscription(), "test-agent", null);
    await service.notifyServerNotification(turnCompletedNotification());
    await service.notifyServerNotification(turnCompletedNotification());

    expect(sent).toHaveLength(1);
    expect(logger.info).toHaveBeenCalledWith(
      "Web Push notification skipped: recently sent.",
      {
        tag: "codex-turn-thread-1-turn-1-completed",
        threadId: "thread-1",
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

    await service.saveSubscription(subscription(), "test-agent", null);
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
