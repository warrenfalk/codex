import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import type { PushSubscription, VapidKeys } from "web-push";

const STORAGE_VERSION = 1;

export type StoredPushSubscription = PushSubscription & {
  createdAt: string;
  updatedAt: string;
  userAgent: string | null;
};

export type PushStorageData = {
  subscriptions: StoredPushSubscription[];
  vapidKeys: VapidKeys | null;
  version: typeof STORAGE_VERSION;
};

function defaultStorageFile(env: NodeJS.ProcessEnv = process.env): string {
  const codexHome = env.CODEX_HOME ?? path.join(os.homedir(), ".codex");
  return path.join(codexHome, "codex-web-push.json");
}

function emptyStorageData(): PushStorageData {
  return {
    subscriptions: [],
    vapidKeys: null,
    version: STORAGE_VERSION,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function isPushSubscription(value: unknown): value is PushSubscription {
  if (!isRecord(value) || typeof value.endpoint !== "string") {
    return false;
  }

  if (!isRecord(value.keys)) {
    return false;
  }

  return (
    typeof value.keys.p256dh === "string" && typeof value.keys.auth === "string"
  );
}

function isVapidKeys(value: unknown): value is VapidKeys {
  return (
    isRecord(value) &&
    typeof value.publicKey === "string" &&
    typeof value.privateKey === "string"
  );
}

function parseStoredSubscription(
  value: unknown,
): StoredPushSubscription | null {
  if (!isPushSubscription(value) || !isRecord(value)) {
    return null;
  }

  const createdAt =
    typeof value.createdAt === "string" ? value.createdAt : new Date().toJSON();
  const updatedAt =
    typeof value.updatedAt === "string" ? value.updatedAt : createdAt;
  const userAgent =
    typeof value.userAgent === "string" ? value.userAgent : null;

  return {
    endpoint: value.endpoint,
    expirationTime:
      typeof value.expirationTime === "number" ? value.expirationTime : null,
    keys: {
      auth: value.keys.auth,
      p256dh: value.keys.p256dh,
    },
    createdAt,
    updatedAt,
    userAgent,
  };
}

function parseStorageData(value: unknown): PushStorageData {
  if (!isRecord(value)) {
    return emptyStorageData();
  }

  const subscriptions = Array.isArray(value.subscriptions)
    ? value.subscriptions
        .map(parseStoredSubscription)
        .filter((subscription): subscription is StoredPushSubscription =>
          Boolean(subscription),
        )
    : [];

  return {
    subscriptions,
    vapidKeys: isVapidKeys(value.vapidKeys) ? value.vapidKeys : null,
    version: STORAGE_VERSION,
  };
}

export class PushStorage {
  private writeQueue: Promise<void> = Promise.resolve();

  constructor(private readonly filePath = defaultStorageFile()) {}

  async read(): Promise<PushStorageData> {
    try {
      const raw = await fs.readFile(this.filePath, "utf8");
      return parseStorageData(JSON.parse(raw));
    } catch (error) {
      if (isNodeError(error) && error.code === "ENOENT") {
        return emptyStorageData();
      }

      throw error;
    }
  }

  async update<T>(
    updater: (data: PushStorageData) => { data: PushStorageData; result: T },
  ): Promise<T> {
    const run = this.writeQueue.then(async () => {
      const current = await this.read();
      const { data, result } = updater(current);
      await this.write(data);
      return result;
    });

    this.writeQueue = run.then(
      () => undefined,
      () => undefined,
    );

    return run;
  }

  private async write(data: PushStorageData): Promise<void> {
    await fs.mkdir(path.dirname(this.filePath), { recursive: true });
    const tempFile = `${this.filePath}.${process.pid}.${Date.now()}.tmp`;
    await fs.writeFile(tempFile, `${JSON.stringify(data, null, 2)}\n`, {
      mode: 0o600,
    });
    await fs.rename(tempFile, this.filePath);
  }
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}
