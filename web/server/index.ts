import fs from "node:fs";
import http from "node:http";
import path from "node:path";

import express from "express";

import { resolveRelayConfig } from "./config.js";
import { attachRelay } from "./relay.js";

function createApp(staticDir: string) {
  const app = express();

  app.get("/healthz", (_request, response) => {
    response.json({ ok: true });
  });

  if (fs.existsSync(staticDir)) {
    app.use(express.static(staticDir));
    app.get("*", (_request, response) => {
      response.sendFile(path.join(staticDir, "index.html"));
    });
  }

  return app;
}

async function main(): Promise<void> {
  const config = resolveRelayConfig();
  const app = createApp(config.staticDir);
  const server = http.createServer(app);

  await attachRelay(server, config.backendUrl.toString());

  await new Promise<void>((resolve) => {
    server.listen(config.port, config.host, () => resolve());
  });

  process.stdout.write(
    `codex-web relay listening on http://127.0.0.1:${config.port}\n`,
  );
  process.stdout.write(
    `proxying websocket traffic to ${config.backendUrl.toString()}\n`,
  );
}

main().catch((error) => {
  process.stderr.write(`${String(error)}\n`);
  process.exitCode = 1;
});
