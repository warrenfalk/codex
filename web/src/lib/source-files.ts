import {
  SOURCE_FILE_READ_PATH,
  type SourceFileErrorResponse,
  type SourceFileKind,
  type SourceFileReadResponse,
} from "./source-file-protocol";

export type SourceFileReadParams = {
  path: string;
  signal?: AbortSignal;
  threadId: string;
};

export class SourceFileReadError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly details: SourceFileErrorResponse | null,
  ) {
    super(message);
    this.name = "SourceFileReadError";
  }
}

function isErrorResponse(value: unknown): value is SourceFileErrorResponse {
  return (
    typeof value === "object" &&
    value !== null &&
    "error" in value &&
    typeof value.error === "string"
  );
}

function isSourceFileKind(value: unknown): value is SourceFileKind {
  return value === "markdown" || value === "source" || value === "text";
}

function isSourceFileReadResponse(
  value: unknown,
): value is SourceFileReadResponse {
  return (
    typeof value === "object" &&
    value !== null &&
    "content" in value &&
    typeof value.content === "string" &&
    "displayPath" in value &&
    typeof value.displayPath === "string" &&
    "fileKind" in value &&
    isSourceFileKind(value.fileKind) &&
    "language" in value &&
    typeof value.language === "string" &&
    "line" in value &&
    (value.line === null ||
      (typeof value.line === "number" && Number.isSafeInteger(value.line))) &&
    "path" in value &&
    typeof value.path === "string" &&
    "root" in value &&
    typeof value.root === "string" &&
    "sizeBytes" in value &&
    typeof value.sizeBytes === "number" &&
    Number.isFinite(value.sizeBytes)
  );
}

export async function readSourceFile({
  path,
  signal,
  threadId,
}: SourceFileReadParams): Promise<SourceFileReadResponse> {
  const params = new URLSearchParams({
    path,
    threadId,
  });
  const response = await fetch(`${SOURCE_FILE_READ_PATH}?${params}`, {
    signal,
  });
  const body: unknown = await response.json().catch(() => null);

  if (!response.ok) {
    const details = isErrorResponse(body) ? body : null;
    throw new SourceFileReadError(
      details?.error ?? "Failed to read source file.",
      response.status,
      details,
    );
  }

  if (!isSourceFileReadResponse(body)) {
    throw new SourceFileReadError(
      "File API returned an invalid response.",
      response.status,
      null,
    );
  }

  return body;
}
