import crypto from "node:crypto";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import express from "express";

export const CONTROL_FILE_VERSION = 1;
export const CONTROL_SHUTDOWN_PATH = "/api/control/shutdown";

export type ControlFileData = {
  baseUrl: string;
  pid: number;
  shutdownUrl: string;
  startedAt: string;
  token: string;
  version: typeof CONTROL_FILE_VERSION;
};

export class ControlFileError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ControlFileError";
  }
}

type ControlRouterOptions = {
  onShutdown: () => Promise<void> | void;
  token: string;
};

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function assertUrl(value: string, field: string): void {
  try {
    new URL(value);
  } catch (error) {
    throw new ControlFileError(
      `Invalid codex web control file: ${field} is not a URL: ${String(error)}`,
    );
  }
}

function parseControlFile(value: unknown): ControlFileData {
  if (!isRecord(value)) {
    throw new ControlFileError(
      "Invalid codex web control file: expected an object",
    );
  }

  if (value.version !== CONTROL_FILE_VERSION) {
    throw new ControlFileError(
      `Unsupported codex web control file version: ${String(value.version)}`,
    );
  }

  const pid = value.pid;
  if (typeof pid !== "number" || !Number.isInteger(pid) || pid <= 0) {
    throw new ControlFileError(
      "Invalid codex web control file: pid is invalid",
    );
  }

  if (typeof value.baseUrl !== "string" || value.baseUrl.length === 0) {
    throw new ControlFileError(
      "Invalid codex web control file: baseUrl is invalid",
    );
  }

  if (typeof value.shutdownUrl !== "string" || value.shutdownUrl.length === 0) {
    throw new ControlFileError(
      "Invalid codex web control file: shutdownUrl is invalid",
    );
  }

  if (typeof value.token !== "string" || value.token.length === 0) {
    throw new ControlFileError(
      "Invalid codex web control file: token is invalid",
    );
  }

  if (typeof value.startedAt !== "string" || value.startedAt.length === 0) {
    throw new ControlFileError(
      "Invalid codex web control file: startedAt is invalid",
    );
  }

  assertUrl(value.baseUrl, "baseUrl");
  assertUrl(value.shutdownUrl, "shutdownUrl");

  return {
    baseUrl: value.baseUrl,
    pid,
    shutdownUrl: value.shutdownUrl,
    startedAt: value.startedAt,
    token: value.token,
    version: CONTROL_FILE_VERSION,
  };
}

export function resolveControlFilePath(
  env: NodeJS.ProcessEnv = process.env,
): string {
  if (env.CODEX_WEB_CONTROL_PATH) {
    return path.resolve(env.CODEX_WEB_CONTROL_PATH);
  }

  const codexHome = env.CODEX_HOME ?? path.join(os.homedir(), ".codex");
  return path.join(codexHome, "codex-web-control.json");
}

export function createControlToken(): string {
  return crypto.randomBytes(32).toString("base64url");
}

export function controlBaseUrl(host: string, port: number): string {
  const reachableHost =
    host === "0.0.0.0" || host.length === 0
      ? "127.0.0.1"
      : host === "::"
        ? "::1"
        : host;
  const urlHost = reachableHost.includes(":")
    ? `[${reachableHost}]`
    : reachableHost;
  return `http://${urlHost}:${port}`;
}

export function buildControlFileData(
  host: string,
  port: number,
  token: string,
  startedAt = new Date(),
): ControlFileData {
  const baseUrl = controlBaseUrl(host, port);
  return {
    baseUrl,
    pid: process.pid,
    shutdownUrl: new URL(CONTROL_SHUTDOWN_PATH, baseUrl).toString(),
    startedAt: startedAt.toISOString(),
    token,
    version: CONTROL_FILE_VERSION,
  };
}

export async function writeControlFile(
  filePath: string,
  data: ControlFileData,
): Promise<void> {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  const tempFile = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  await fs.writeFile(tempFile, `${JSON.stringify(data, null, 2)}\n`, {
    mode: 0o600,
  });
  await fs.rename(tempFile, filePath);
}

export async function removeControlFile(filePath: string): Promise<void> {
  await fs.rm(filePath, { force: true });
}

export async function readControlFile(
  filePath: string,
): Promise<ControlFileData> {
  let raw: string;
  try {
    raw = await fs.readFile(filePath, "utf8");
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      throw new ControlFileError(
        `No codex web control file found at ${filePath}`,
      );
    }
    throw error;
  }

  try {
    return parseControlFile(JSON.parse(raw));
  } catch (error) {
    if (error instanceof SyntaxError) {
      throw new ControlFileError(
        `Invalid codex web control file at ${filePath}: ${error.message}`,
      );
    }
    throw error;
  }
}

function bearerTokenFromHeader(header: string | undefined): string | null {
  const prefix = "Bearer ";
  if (!header?.startsWith(prefix)) {
    return null;
  }

  return header.slice(prefix.length);
}

function tokenMatches(actual: string, expected: string): boolean {
  const actualBuffer = Buffer.from(actual);
  const expectedBuffer = Buffer.from(expected);
  return (
    actualBuffer.length === expectedBuffer.length &&
    crypto.timingSafeEqual(actualBuffer, expectedBuffer)
  );
}

export function createControlRouter({
  onShutdown,
  token,
}: ControlRouterOptions): express.Router {
  const router = express.Router();
  let shutdownStarted = false;

  router.post(CONTROL_SHUTDOWN_PATH, (request, response) => {
    const authorization = request.get("authorization");
    const requestToken = bearerTokenFromHeader(authorization);

    if (!requestToken) {
      response.status(401).json({
        error: "missing bearer token",
        ok: false,
      });
      return;
    }

    if (!tokenMatches(requestToken, token)) {
      response.status(403).json({
        error: "invalid bearer token",
        ok: false,
      });
      return;
    }

    const shouldStartShutdown = !shutdownStarted;
    shutdownStarted = true;
    response.json({ ok: true, shuttingDown: true });

    if (shouldStartShutdown) {
      response.once("finish", () => {
        setImmediate(() => {
          void Promise.resolve(onShutdown()).catch((error: unknown) => {
            console.error("Failed to shut down codex web server.", error);
          });
        });
      });
    }
  });

  return router;
}
