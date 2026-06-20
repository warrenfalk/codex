export type SourceFileRouteOptions = {
  root?: string | null;
  threadId: string;
};

const externalSchemePattern = /^[a-z][a-z0-9+.-]*:/iu;
const appPathPrefixes = ["/api/", "/rpc", "/threads/", "/files", "/healthz"];

function isLikelyRelativeFilePath(path: string): boolean {
  return (
    path.startsWith("./") ||
    path.startsWith("../") ||
    path.includes("/") ||
    /\.[A-Za-z0-9][A-Za-z0-9_-]*(?::[1-9]\d*)?$/u.test(path)
  );
}

function parseLineSuffix(path: string): { line: string | null; path: string } {
  const match = path.match(/^(.*):([1-9]\d*)$/u);
  if (!match) {
    return { line: null, path };
  }

  return {
    line: match[2] ?? null,
    path: match[1] ?? path,
  };
}

function pathFromHref(href: string, root: string | null | undefined) {
  const trimmed = href.trim();
  if (!trimmed || trimmed.startsWith("#") || trimmed.startsWith("?")) {
    return null;
  }

  if (trimmed.startsWith("file://")) {
    try {
      return decodeURIComponent(new URL(trimmed).pathname);
    } catch {
      return null;
    }
  }

  if (externalSchemePattern.test(trimmed)) {
    return null;
  }

  if (trimmed.startsWith("/")) {
    if (appPathPrefixes.some((prefix) => trimmed.startsWith(prefix))) {
      return null;
    }
    if (root && !trimmed.startsWith(root)) {
      return null;
    }
    return trimmed;
  }

  return isLikelyRelativeFilePath(trimmed) ? trimmed : null;
}

export function sourceFileRouteFromHref(
  href: string | undefined,
  { root, threadId }: SourceFileRouteOptions,
): string | null {
  if (!href) {
    return null;
  }

  const rawPath = pathFromHref(href, root);
  if (!rawPath) {
    return null;
  }

  const { line, path } = parseLineSuffix(rawPath);
  const params = new URLSearchParams({
    path,
    threadId,
  });
  if (line) {
    params.set("line", line);
  }

  return `/files?${params}`;
}
