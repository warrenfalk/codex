import { describe, expect, it } from "vitest";

import { DEFAULT_BACKEND_WS_URL, resolveRelayConfig } from "./config.js";

describe("resolveRelayConfig", () => {
  it("defaults to the local backend websocket URL", () => {
    const config = resolveRelayConfig({});

    expect(config.backendUrl.toString()).toBe(`${DEFAULT_BACKEND_WS_URL}/`);
    expect(config.port).toBe(4201);
  });

  it("uses the UI port by default in production", () => {
    const config = resolveRelayConfig({ NODE_ENV: "production" });

    expect(config.port).toBe(4200);
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
      CODEX_WEB_BACKEND_WS_URL: "ws://127.0.0.1:8080",
      CODEX_WEB_PROXY_PORT: "3555",
    });

    expect(config.backendUrl.toString()).toBe("ws://127.0.0.1:8080/");
    expect(config.port).toBe(3555);
  });
});
