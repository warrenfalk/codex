import express from "express";
import { createRequire } from "node:module";
import type {
  PushSubscription,
  RequestOptions,
  SendResult,
  VapidKeys,
} from "web-push";

import type { JsonRpcRequestMessage } from "../src/lib/jsonrpc";
import type { ServerNotification } from "../src/types/protocol";

import {
  pushMessageForServerNotification,
  pushMessageForServerRequest,
  type BrowserPushMessage,
} from "./push-notifications.js";
import { isPushSubscription, PushStorage } from "./push-store.js";

const DEFAULT_VAPID_SUBJECT = "mailto:codex-web@localhost";
const RECENT_NOTIFICATION_TTL_MS = 10 * 60 * 1000;
const require = createRequire(import.meta.url);
const webPush = require("web-push") as typeof import("web-push");

export type PushNotifier = {
  notifyServerNotification(notification: ServerNotification): Promise<void>;
  notifyServerRequest(request: JsonRpcRequestMessage): Promise<void>;
};

type PushSender = (
  subscription: PushSubscription,
  payload: string,
  options: RequestOptions,
) => Promise<SendResult>;

type PushLogger = Pick<Console, "info" | "warn">;

function isGonePushError(error: unknown): boolean {
  return (
    error instanceof webPush.WebPushError &&
    (error.statusCode === 404 || error.statusCode === 410)
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export class PushNotificationService implements PushNotifier {
  private readonly recentNotifications = new Map<string, number>();

  constructor(
    private readonly store = new PushStorage(),
    private readonly vapidSubject = process.env.CODEX_WEB_PUSH_VAPID_SUBJECT ??
      DEFAULT_VAPID_SUBJECT,
    private readonly sendNotification: PushSender = (
      subscription,
      payload,
      options,
    ) => webPush.sendNotification(subscription, payload, options),
    private readonly generateVapidKeys: () => VapidKeys = webPush.generateVAPIDKeys,
    private readonly logger: PushLogger = console,
  ) {}

  async getPublicKey(): Promise<string> {
    return (await this.ensureVapidKeys()).publicKey;
  }

  async saveSubscription(
    subscription: PushSubscription,
    userAgent: string | null,
  ): Promise<void> {
    await this.store.update((data) => {
      const now = new Date().toJSON();
      const existing = data.subscriptions.find(
        (current) => current.endpoint === subscription.endpoint,
      );
      const createdAt = existing?.createdAt ?? now;
      const subscriptions = [
        ...data.subscriptions.filter(
          (current) => current.endpoint !== subscription.endpoint,
        ),
        {
          ...subscription,
          createdAt,
          updatedAt: now,
          userAgent,
        },
      ];

      return {
        data: {
          ...data,
          subscriptions,
        },
        result: undefined,
      };
    });
  }

  async deleteSubscription(endpoint: string): Promise<void> {
    await this.store.update((data) => ({
      data: {
        ...data,
        subscriptions: data.subscriptions.filter(
          (subscription) => subscription.endpoint !== endpoint,
        ),
      },
      result: undefined,
    }));
  }

  async notifyServerRequest(request: JsonRpcRequestMessage): Promise<void> {
    await this.sendPushMessage(pushMessageForServerRequest(request));
  }

  async notifyServerNotification(
    notification: ServerNotification,
  ): Promise<void> {
    await this.sendPushMessage(pushMessageForServerNotification(notification));
  }

  private async sendPushMessage(
    message: BrowserPushMessage | null,
  ): Promise<void> {
    if (!message) {
      return;
    }

    if (!this.shouldSend(message.tag)) {
      this.logger.info("Web Push notification skipped: recently sent.", {
        tag: message.tag,
        title: message.title,
        url: message.url,
      });
      return;
    }

    const data = await this.store.read();
    if (data.subscriptions.length === 0) {
      this.logger.info("Web Push notification skipped: no subscriptions.", {
        tag: message.tag,
        title: message.title,
        url: message.url,
      });
      return;
    }

    const vapidKeys = await this.ensureVapidKeys();
    const payload = JSON.stringify({
      ...message,
      timestamp: Date.now(),
    });
    const expiredEndpoints: string[] = [];
    let failedCount = 0;
    let sentCount = 0;

    this.logger.info("Sending Web Push notification.", {
      subscriptions: data.subscriptions.length,
      tag: message.tag,
      title: message.title,
      url: message.url,
    });

    await Promise.all(
      data.subscriptions.map(async (subscription) => {
        try {
          await this.sendNotification(subscription, payload, {
            TTL: 60 * 60,
            urgency: "high",
            vapidDetails: {
              privateKey: vapidKeys.privateKey,
              publicKey: vapidKeys.publicKey,
              subject: this.vapidSubject,
            },
          });
          sentCount += 1;
        } catch (error) {
          if (isGonePushError(error)) {
            expiredEndpoints.push(subscription.endpoint);
            return;
          }

          failedCount += 1;
          this.logger.warn("Failed to send Web Push notification.", error);
        }
      }),
    );

    if (expiredEndpoints.length > 0) {
      await this.removeEndpoints(expiredEndpoints);
    }

    this.logger.info("Finished Web Push notification.", {
      expired: expiredEndpoints.length,
      failed: failedCount,
      sent: sentCount,
      tag: message.tag,
    });
  }

  private shouldSend(tag: string): boolean {
    const now = Date.now();

    for (const [key, sentAt] of this.recentNotifications) {
      if (now - sentAt > RECENT_NOTIFICATION_TTL_MS) {
        this.recentNotifications.delete(key);
      }
    }

    const lastSentAt = this.recentNotifications.get(tag);
    if (lastSentAt && now - lastSentAt <= RECENT_NOTIFICATION_TTL_MS) {
      return false;
    }

    this.recentNotifications.set(tag, now);
    return true;
  }

  private async ensureVapidKeys(): Promise<VapidKeys> {
    return this.store.update((data) => {
      if (data.vapidKeys) {
        return {
          data,
          result: data.vapidKeys,
        };
      }

      const vapidKeys = this.generateVapidKeys();
      return {
        data: {
          ...data,
          vapidKeys,
        },
        result: vapidKeys,
      };
    });
  }

  private async removeEndpoints(endpoints: string[]): Promise<void> {
    const endpointSet = new Set(endpoints);
    await this.store.update((data) => ({
      data: {
        ...data,
        subscriptions: data.subscriptions.filter(
          (subscription) => !endpointSet.has(subscription.endpoint),
        ),
      },
      result: undefined,
    }));
  }
}

export function createPushRouter(pushService: PushNotificationService) {
  const router = express.Router();

  router.get("/vapid-public-key", async (_request, response, next) => {
    try {
      response.json({ publicKey: await pushService.getPublicKey() });
    } catch (error) {
      next(error);
    }
  });

  router.post("/subscriptions", async (request, response, next) => {
    try {
      const subscription = isRecord(request.body)
        ? request.body.subscription
        : null;
      if (!isPushSubscription(subscription)) {
        response.status(400).json({ error: "Invalid push subscription" });
        return;
      }

      await pushService.saveSubscription(
        subscription,
        request.get("user-agent") ?? null,
      );
      response.status(204).end();
    } catch (error) {
      next(error);
    }
  });

  router.delete("/subscriptions", async (request, response, next) => {
    try {
      const endpoint = isRecord(request.body) ? request.body.endpoint : null;
      if (typeof endpoint !== "string") {
        response.status(400).json({ error: "Invalid push endpoint" });
        return;
      }

      await pushService.deleteSubscription(endpoint);
      response.status(204).end();
    } catch (error) {
      next(error);
    }
  });

  return router;
}
