import path from "node:path";
import { fileURLToPath } from "node:url";

export const DEFAULT_BACKEND_URL = "unix://";
export const DEFAULT_DEV_HOST = "0.0.0.0";
export const DEFAULT_DEV_UI_PORT = 4202;
export const DEFAULT_DEV_PROXY_PORT = 4203;
export const DEFAULT_PROD_HOST = "127.0.0.1";
export const DEFAULT_PROD_UI_PORT = 4200;

export type RelayConfig = {
  backendUrl: string;
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
      ? parsePort(env.CODEX_WEB_UI_PORT, DEFAULT_PROD_UI_PORT)
      : DEFAULT_DEV_PROXY_PORT;

  return parsePort(env.CODEX_WEB_PROXY_PORT, defaultPort);
}

function resolveHost(env: NodeJS.ProcessEnv): string {
  if (env.CODEX_WEB_HOST) {
    return env.CODEX_WEB_HOST;
  }

  return env.NODE_ENV === "production" ? DEFAULT_PROD_HOST : DEFAULT_DEV_HOST;
}

function resolveBackendUrl(env: NodeJS.ProcessEnv): string {
  const backendRaw =
    env.CODEX_WEB_BACKEND_URL ??
    env.CODEX_WEB_BACKEND_WS_URL ??
    DEFAULT_BACKEND_URL;

  if (backendRaw.startsWith("unix://")) {
    return backendRaw;
  }

  let backendUrl: URL;
  try {
    backendUrl = new URL(backendRaw);
  } catch (error) {
    throw new Error(
      `CODEX_WEB_BACKEND_URL is not a valid URL: ${String(error)}`,
    );
  }

  if (!["ws:", "wss:"].includes(backendUrl.protocol)) {
    throw new Error(
      "CODEX_WEB_BACKEND_URL must use unix://, ws://, or wss:// protocol",
    );
  }

  return backendUrl.toString();
}

export function resolveRelayConfig(
  env: NodeJS.ProcessEnv = process.env,
): RelayConfig {
  const projectRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "..",
  );

  return {
    backendUrl: resolveBackendUrl(env),
    host: resolveHost(env),
    port: resolvePort(env),
    projectRoot,
    staticDir: path.join(projectRoot, "dist"),
  };
}
