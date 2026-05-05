import path from "node:path";
import { fileURLToPath } from "node:url";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

import {
  DEFAULT_DEV_PROXY_PORT,
  DEFAULT_DEV_UI_PORT,
} from "./server/config";

const projectRoot = path.dirname(fileURLToPath(import.meta.url));
const devHost = process.env.CODEX_WEB_HOST ?? "127.0.0.1";
const allowedHosts = ["agent.warrenfalk.com"];
const relayPort = Number(
  process.env.CODEX_WEB_PROXY_PORT ?? DEFAULT_DEV_PROXY_PORT,
);
const uiPort = Number(process.env.CODEX_WEB_UI_PORT ?? DEFAULT_DEV_UI_PORT);

export default defineConfig({
  plugins: [react()],
  server: {
    allowedHosts,
    host: devHost,
    port: uiPort,
    strictPort: true,
    proxy: {
      "/rpc": {
        target: `http://127.0.0.1:${relayPort}`,
        ws: true,
      },
      "/healthz": {
        target: `http://127.0.0.1:${relayPort}`,
      },
    },
  },
  preview: {
    allowedHosts,
    host: devHost,
    port: uiPort,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": path.resolve(projectRoot, "src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      reportsDirectory: "./coverage",
    },
  },
});
