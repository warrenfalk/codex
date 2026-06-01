import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  ControlFileError,
  readControlFile,
  resolveControlFilePath,
} from "./control.js";

type FetchLike = (
  input: string,
  init: {
    headers: Record<string, string>;
    method: "POST";
  },
) => Promise<Response>;

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}

function isProcessRunning(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return isNodeError(error) && error.code === "EPERM";
  }
}

async function responseText(response: Response): Promise<string> {
  try {
    return await response.text();
  } catch {
    return "";
  }
}

export async function requestShutdown(
  env: NodeJS.ProcessEnv = process.env,
  fetchImpl: FetchLike = fetch,
): Promise<void> {
  const controlPath = resolveControlFilePath(env);
  const control = await readControlFile(controlPath);

  if (!isProcessRunning(control.pid)) {
    throw new ControlFileError(
      `Stale codex web control file at ${controlPath}: process ${control.pid} is not running`,
    );
  }

  let response: Response;
  try {
    response = await fetchImpl(control.shutdownUrl, {
      headers: {
        authorization: `Bearer ${control.token}`,
      },
      method: "POST",
    });
  } catch (error) {
    throw new Error(`Shutdown request failed: ${String(error)}`);
  }

  if (!response.ok) {
    const body = await responseText(response);
    throw new Error(
      `Shutdown request failed with HTTP ${response.status}${
        body.length > 0 ? `: ${body.slice(0, 500)}` : ""
      }`,
    );
  }
}

function isMainModule(): boolean {
  return (
    process.argv[1] !== undefined &&
    fileURLToPath(import.meta.url) === path.resolve(process.argv[1])
  );
}

if (isMainModule()) {
  requestShutdown().catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`server:shutdown failed: ${message}\n`);
    process.exitCode = 1;
  });
}
