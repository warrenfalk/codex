import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

import express from "express";

import {
  buildControlFileData,
  createControlRouter,
  createControlToken,
  removeControlFile,
  resolveControlFilePath,
  writeControlFile,
  type ControlFileData,
} from "./control.js";
import { resolveRelayConfig, type RelayConfig } from "./config.js";
import { createPushRouter, PushNotificationService } from "./push-service.js";
import { attachRelay, type RelayHandle } from "./relay.js";

export type RunningWebServer = {
  config: RelayConfig;
  control: ControlFileData;
  controlFilePath: string;
  server: http.Server;
  shutdown: () => Promise<void>;
};

function createApp(
  staticDir: string,
  pushService: PushNotificationService,
  controlRouter: express.Router,
) {
  const app = express();

  app.get("/healthz", (_request, response) => {
    response.json({ ok: true });
  });

  app.use(controlRouter);

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

async function listen(server: http.Server, config: RelayConfig): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const onError = (error: Error) => {
      server.off("listening", onListening);
      reject(error);
    };
    const onListening = () => {
      server.off("error", onError);
      resolve();
    };

    server.once("error", onError);
    server.listen(config.port, config.host, onListening);
  });
}

function listeningPort(server: http.Server, fallbackPort: number): number {
  const address = server.address();
  if (typeof address === "object" && address !== null) {
    return address.port;
  }

  return fallbackPort;
}

async function closeHttpServer(server: http.Server): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (
        error &&
        (error as NodeJS.ErrnoException).code !== "ERR_SERVER_NOT_RUNNING"
      ) {
        reject(error);
        return;
      }
      resolve();
    });
    server.closeIdleConnections();
  });
}

export async function startServer(
  env: NodeJS.ProcessEnv = process.env,
): Promise<RunningWebServer> {
  const config = resolveRelayConfig(env);
  const controlFilePath = resolveControlFilePath(env);
  const token = createControlToken();
  const pushService = new PushNotificationService();
  let relay: RelayHandle | null = null;
  let server: http.Server | null = null;
  let shutdownPromise: Promise<void> | null = null;
  let control: ControlFileData | null = null;

  const shutdown = (): Promise<void> => {
    if (shutdownPromise) {
      return shutdownPromise;
    }

    shutdownPromise = (async () => {
      if (relay) {
        await relay.close();
        relay = null;
      }

      if (server) {
        await closeHttpServer(server);
      }

      if (control) {
        await removeControlFile(controlFilePath);
        control = null;
      }
    })();

    return shutdownPromise;
  };

  const app = createApp(
    config.staticDir,
    pushService,
    createControlRouter({ onShutdown: shutdown, token }),
  );
  server = http.createServer(app);

  try {
    relay = await attachRelay(server, config.backendUrl, {
      pushNotifier: pushService,
    });

    await listen(server, config);
    const nextControl = buildControlFileData(
      config.host,
      listeningPort(server, config.port),
      token,
    );
    await writeControlFile(controlFilePath, nextControl);
    control = nextControl;
  } catch (error) {
    await shutdown().catch(() => undefined);
    throw error;
  }

  if (!control) {
    throw new Error("codex web control file was not initialized");
  }

  return {
    config,
    control,
    controlFilePath,
    server,
    shutdown,
  };
}

function isMainModule(): boolean {
  return (
    process.argv[1] !== undefined &&
    fileURLToPath(import.meta.url) === path.resolve(process.argv[1])
  );
}

async function main(): Promise<void> {
  const running = await startServer();

  process.stdout.write(
    `codex-web relay listening on ${running.control.baseUrl}\n`,
  );
  process.stdout.write(
    `proxying websocket traffic to ${running.config.backendUrl}\n`,
  );

  const shutdownFromSignal = () => {
    void running.shutdown().catch((error: unknown) => {
      process.stderr.write(`${String(error)}\n`);
      process.exitCode = 1;
    });
  };
  process.once("SIGINT", shutdownFromSignal);
  process.once("SIGTERM", shutdownFromSignal);
}

if (isMainModule()) {
  main().catch((error) => {
    process.stderr.write(`${String(error)}\n`);
    process.exitCode = 1;
  });
}
