import fs from "node:fs/promises";
import path from "node:path";
import { TextDecoder } from "node:util";

import express from "express";

import {
  SOURCE_FILE_READ_PATH,
  type SourceFileErrorResponse,
  type SourceFileKind,
  type SourceFileReadResponse,
} from "../src/lib/source-file-protocol.js";
import type { Thread } from "../src/types/protocol";

const SOURCE_FILE_MAX_BYTES = 1024 * 1024;

const extensionLanguages = new Map<string, string>([
  [".bash", "bash"],
  [".c", "c"],
  [".cc", "cpp"],
  [".cjs", "js"],
  [".clj", "clojure"],
  [".cljs", "clojure"],
  [".cpp", "cpp"],
  [".cs", "csharp"],
  [".css", "css"],
  [".cts", "ts"],
  [".cxx", "cpp"],
  [".edn", "clojure"],
  [".erl", "erlang"],
  [".ex", "elixir"],
  [".exs", "elixir"],
  [".fish", "fish"],
  [".fs", "fsharp"],
  [".fsi", "fsharp"],
  [".fsx", "fsharp"],
  [".gif", "gif"],
  [".go", "go"],
  [".gql", "graphql"],
  [".graphql", "graphql"],
  [".h", "c"],
  [".hpp", "cpp"],
  [".hrl", "erlang"],
  [".html", "html"],
  [".java", "java"],
  [".jpeg", "jpeg"],
  [".jpg", "jpeg"],
  [".js", "js"],
  [".json", "json"],
  [".jsonl", "jsonl"],
  [".jsx", "jsx"],
  [".kt", "kotlin"],
  [".kts", "kotlin"],
  [".lua", "lua"],
  [".mjs", "js"],
  [".mts", "ts"],
  [".nix", "nix"],
  [".php", "php"],
  [".png", "png"],
  [".proto", "proto"],
  [".py", "py"],
  [".r", "r"],
  [".rb", "ruby"],
  [".rs", "rust"],
  [".sass", "sass"],
  [".scss", "scss"],
  [".sh", "bash"],
  [".sql", "sql"],
  [".svg", "svg"],
  [".svelte", "svelte"],
  [".swift", "swift"],
  [".toml", "toml"],
  [".ts", "ts"],
  [".tsx", "tsx"],
  [".vue", "vue"],
  [".webp", "webp"],
  [".xml", "xml"],
  [".yaml", "yaml"],
  [".yml", "yaml"],
  [".zsh", "zsh"],
]);

const imageExtensions = new Set([
  ".gif",
  ".jpeg",
  ".jpg",
  ".png",
  ".svg",
  ".webp",
]);

const unsupportedExtensions = new Set([
  ".7z",
  ".avif",
  ".bin",
  ".bz2",
  ".class",
  ".dll",
  ".dmg",
  ".exe",
  ".gz",
  ".ico",
  ".jar",
  ".lockb",
  ".o",
  ".pdf",
  ".so",
  ".tar",
  ".wasm",
  ".zip",
]);

const specialNameLanguages = new Map<string, string>([
  ["dockerfile", "dockerfile"],
  ["flake.lock", "json"],
  ["gemfile", "ruby"],
  ["justfile", "just"],
  ["makefile", "makefile"],
  ["rakefile", "ruby"],
]);

export type SourceFileRouterOptions = {
  getThread: (threadId: string) => Thread | null;
};

function stringQueryParam(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function parseLine(value: unknown): number | null {
  if (typeof value !== "string" || !/^[1-9]\d*$/.test(value)) {
    return null;
  }

  const line = Number(value);
  return Number.isSafeInteger(line) ? line : null;
}

function parsePathAndLine(rawPath: string, rawLine: unknown) {
  const lineFromQuery = parseLine(rawLine);
  if (lineFromQuery !== null) {
    return { line: lineFromQuery, sourcePath: rawPath };
  }

  const match = rawPath.match(/^(.*):([1-9]\d*)$/u);
  if (!match) {
    return { line: null, sourcePath: rawPath };
  }

  const sourcePath = match[1] ?? rawPath;
  const line = Number(match[2]);
  return {
    line: Number.isSafeInteger(line) ? line : null,
    sourcePath,
  };
}

function isInsideRoot(root: string, candidate: string): boolean {
  const relative = path.relative(root, candidate);
  return (
    relative === "" ||
    (!relative.startsWith("..") && !path.isAbsolute(relative))
  );
}

function displayPath(root: string, filePath: string): string {
  const relative = path.relative(root, filePath);
  return relative && !relative.startsWith("..") ? relative : filePath;
}

function classifyFile(filePath: string): {
  fileKind: SourceFileKind | "image" | "unsupported";
  language: string;
} {
  const basename = path.basename(filePath).toLowerCase();
  const extension = path.extname(filePath).toLowerCase();
  const specialLanguage = specialNameLanguages.get(basename);
  const language = specialLanguage ?? extensionLanguages.get(extension);

  if (basename.endsWith(".md") || basename.endsWith(".markdown")) {
    return { fileKind: "markdown", language: "markdown" };
  }

  if (imageExtensions.has(extension)) {
    return { fileKind: "image", language: language ?? "text" };
  }

  if (unsupportedExtensions.has(extension)) {
    return { fileKind: "unsupported", language: language ?? "text" };
  }

  return {
    fileKind: language ? "source" : "text",
    language: language ?? "text",
  };
}

function errorResponse(
  error: string,
  fileKind?: SourceFileErrorResponse["fileKind"],
  language?: string,
): SourceFileErrorResponse {
  return {
    error,
    ...(fileKind ? { fileKind } : {}),
    ...(language ? { language } : {}),
  };
}

async function resolveSourcePath(root: string, requestedPath: string) {
  if (requestedPath.includes("\0")) {
    throw Object.assign(new Error("Invalid file path."), { status: 400 });
  }

  const absoluteRoot = path.resolve(root);
  const resolvedRoot = await fs.realpath(absoluteRoot);
  const candidate = path.isAbsolute(requestedPath)
    ? path.resolve(requestedPath)
    : path.resolve(resolvedRoot, requestedPath);

  if (!isInsideRoot(resolvedRoot, candidate)) {
    throw Object.assign(new Error("File path is outside the thread root."), {
      status: 403,
    });
  }

  let resolvedFile: string;
  try {
    resolvedFile = await fs.realpath(candidate);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      throw Object.assign(new Error("File not found."), { status: 404 });
    }
    throw error;
  }

  if (!isInsideRoot(resolvedRoot, resolvedFile)) {
    throw Object.assign(new Error("File path is outside the thread root."), {
      status: 403,
    });
  }

  return { resolvedFile, resolvedRoot };
}

export function createSourceFileRouter({ getThread }: SourceFileRouterOptions) {
  const router = express.Router();

  router.get(SOURCE_FILE_READ_PATH, async (request, response) => {
    const threadId = stringQueryParam(request.query.threadId);
    const rawPath = stringQueryParam(request.query.path);
    if (!threadId || !rawPath) {
      response
        .status(400)
        .json(errorResponse("threadId and path are required."));
      return;
    }

    const thread = getThread(threadId);
    if (!thread) {
      response.status(404).json(errorResponse("Thread not found."));
      return;
    }

    if (!path.isAbsolute(thread.cwd)) {
      response.status(400).json(errorResponse("Thread cwd is not absolute."));
      return;
    }

    const { line, sourcePath } = parsePathAndLine(rawPath, request.query.line);

    try {
      const { resolvedFile, resolvedRoot } = await resolveSourcePath(
        thread.cwd,
        sourcePath,
      );
      const stats = await fs.stat(resolvedFile);
      if (!stats.isFile()) {
        response.status(400).json(errorResponse("Path is not a file."));
        return;
      }

      const { fileKind, language } = classifyFile(resolvedFile);
      if (fileKind === "image" || fileKind === "unsupported") {
        response
          .status(415)
          .json(
            errorResponse("File preview is not supported.", fileKind, language),
          );
        return;
      }

      if (stats.size > SOURCE_FILE_MAX_BYTES) {
        response
          .status(413)
          .json(errorResponse("File is too large to preview."));
        return;
      }

      const buffer = await fs.readFile(resolvedFile);
      if (buffer.includes(0)) {
        response
          .status(415)
          .json(errorResponse("Binary file preview is not supported."));
        return;
      }

      let content: string;
      try {
        content = new TextDecoder("utf-8", { fatal: true }).decode(buffer);
      } catch {
        response
          .status(415)
          .json(errorResponse("File is not valid UTF-8 text."));
        return;
      }

      const body: SourceFileReadResponse = {
        content,
        displayPath: displayPath(resolvedRoot, resolvedFile),
        fileKind,
        language,
        line,
        path: resolvedFile,
        root: resolvedRoot,
        sizeBytes: stats.size,
      };
      response.json(body);
    } catch (error) {
      const status =
        typeof error === "object" &&
        error !== null &&
        "status" in error &&
        typeof error.status === "number"
          ? error.status
          : 500;
      response.status(status).json(errorResponse((error as Error).message));
    }
  });

  return router;
}
