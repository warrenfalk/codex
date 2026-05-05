import fs from "node:fs";
import http from "node:http";
import path from "node:path";

import express from "express";

import { resolveRelayConfig } from "./config.js";
import { createPushRouter, PushNotificationService } from "./push-service.js";
import { attachRelay } from "./relay.js";

function createApp(staticDir: string, pushService: PushNotificationService) {
  const app = express();

  app.get("/healthz", (_request, response) => {
    response.json({ ok: true });
  });

  app.use("/api/push", express.json({ limit: "64kb" }));
  app.use("/api/push", createPushRouter(pushService));

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
  const pushService = new PushNotificationService();
  const app = createApp(config.staticDir, pushService);
  const server = http.createServer(app);

  await attachRelay(server, config.backendUrl.toString(), {
    pushNotifier: pushService,
  });

  await new Promise<void>((resolve) => {
    server.listen(config.port, config.host, () => resolve());
  });

  process.stdout.write(
    `codex-web relay listening on http://${config.host}:${config.port}\n`,
  );
  process.stdout.write(
    `proxying websocket traffic to ${config.backendUrl.toString()}\n`,
  );
}

main().catch((error) => {
  process.stderr.write(`${String(error)}\n`);
  process.exitCode = 1;
});
