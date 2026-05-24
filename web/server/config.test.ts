import { describe, expect, it } from "vitest";

import {
  DEFAULT_BACKEND_URL,
  DEFAULT_DEV_HOST,
  DEFAULT_DEV_PROXY_PORT,
  DEFAULT_PROD_HOST,
  DEFAULT_PROD_UI_PORT,
  resolveRelayConfig,
} from "./config.js";

describe("resolveRelayConfig", () => {
  it("defaults to the local backend unix socket URL", () => {
    const config = resolveRelayConfig({});

    expect(config.backendUrl).toBe(DEFAULT_BACKEND_URL);
    expect(config.port).toBe(DEFAULT_DEV_PROXY_PORT);
  });

  it("listens on all interfaces by default during development", () => {
    const config = resolveRelayConfig({});

    expect(config.host).toBe(DEFAULT_DEV_HOST);
  });

  it("uses the UI port by default in production", () => {
    const config = resolveRelayConfig({ NODE_ENV: "production" });

    expect(config.port).toBe(DEFAULT_PROD_UI_PORT);
  });

  it("listens on localhost by default in production", () => {
    const config = resolveRelayConfig({ NODE_ENV: "production" });

    expect(config.host).toBe(DEFAULT_PROD_HOST);
  });

  it("accepts a production UI port", () => {
    const config = resolveRelayConfig({
      CODEX_WEB_UI_PORT: "3555",
      NODE_ENV: "production",
    });

    expect(config.port).toBe(3555);
  });

  it("accepts a websocket backend URL", () => {
    const config = resolveRelayConfig({
      CODEX_WEB_BACKEND_URL: "ws://127.0.0.1:8080",
      CODEX_WEB_PROXY_PORT: "3555",
    });

    expect(config.backendUrl).toBe("ws://127.0.0.1:8080/");
    expect(config.port).toBe(3555);
  });

  it("accepts a custom unix backend socket path", () => {
    const config = resolveRelayConfig({
      CODEX_WEB_BACKEND_URL: "unix:///tmp/codex.sock",
    });

    expect(config.backendUrl).toBe("unix:///tmp/codex.sock");
  });

  it("accepts the legacy websocket backend environment variable", () => {
    const config = resolveRelayConfig({
      CODEX_WEB_BACKEND_WS_URL: "ws://127.0.0.1:8080",
    });

    expect(config.backendUrl).toBe("ws://127.0.0.1:8080/");
  });

  it("accepts a custom host", () => {
    const config = resolveRelayConfig({ CODEX_WEB_HOST: "0.0.0.0" });

    expect(config.host).toBe("0.0.0.0");
  });
});
