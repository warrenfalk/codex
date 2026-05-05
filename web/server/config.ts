import path from "node:path";
import { fileURLToPath } from "node:url";

export const DEFAULT_BACKEND_WS_URL = "ws://127.0.0.1:4222";

export type RelayConfig = {
  backendUrl: URL;
  host: string;
  port: number;
  projectRoot: string;
  staticDir: string;
};

function parsePort(raw: string | undefined, fallback: number): number {
  if (!raw) {
    return fallback;
  }

  const value = Number(raw);
  if (!Number.isInteger(value) || value <= 0) {
    throw new Error(`Invalid port: ${raw}`);
  }

  return value;
}

function resolvePort(env: NodeJS.ProcessEnv): number {
  const defaultPort =
    env.NODE_ENV === "production"
      ? parsePort(env.CODEX_WEB_UI_PORT, 4200)
      : 4201;

  return parsePort(env.CODEX_WEB_PROXY_PORT, defaultPort);
}

export function resolveRelayConfig(
  env: NodeJS.ProcessEnv = process.env,
): RelayConfig {
  const projectRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "..",
  );
  const backendRaw = env.CODEX_WEB_BACKEND_WS_URL ?? DEFAULT_BACKEND_WS_URL;

  let backendUrl: URL;
  try {
    backendUrl = new URL(backendRaw);
  } catch (error) {
    throw new Error(
      `CODEX_WEB_BACKEND_WS_URL is not a valid URL: ${String(error)}`,
    );
  }

  if (!["ws:", "wss:"].includes(backendUrl.protocol)) {
    throw new Error(
      "CODEX_WEB_BACKEND_WS_URL must use ws:// or wss:// protocol",
    );
  }

  return {
    backendUrl,
    host: process.env.CODEX_WEB_HOST ?? "127.0.0.1",
    port: resolvePort(env),
    projectRoot,
    staticDir: path.join(projectRoot, "dist"),
  };
}
