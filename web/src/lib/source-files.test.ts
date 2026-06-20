import { afterEach, describe, expect, it, vi } from "vitest";

import { readSourceFile, SourceFileReadError } from "./source-files";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("readSourceFile", () => {
  it("returns validated source file responses", async () => {
    const fetch = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          content: "const value = 1;\n",
          displayPath: "src/app.ts",
          fileKind: "source",
          language: "ts",
          line: 1,
          path: "/workspace/src/app.ts",
          root: "/workspace",
          sizeBytes: 17,
        }),
      ),
    );
    vi.stubGlobal("fetch", fetch);

    await expect(
      readSourceFile({ path: "src/app.ts", threadId: "thread-1" }),
    ).resolves.toEqual({
      content: "const value = 1;\n",
      displayPath: "src/app.ts",
      fileKind: "source",
      language: "ts",
      line: 1,
      path: "/workspace/src/app.ts",
      root: "/workspace",
      sizeBytes: 17,
    });
    expect(fetch).toHaveBeenCalledWith(
      "/api/files/read?path=src%2Fapp.ts&threadId=thread-1",
      { signal: undefined },
    );
  });

  it("throws server error responses", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: "Thread not found." }), {
          status: 404,
        }),
      ),
    );

    await expect(
      readSourceFile({ path: "src/app.ts", threadId: "thread-1" }),
    ).rejects.toEqual(
      new SourceFileReadError("Thread not found.", 404, {
        error: "Thread not found.",
      }),
    );
  });

  it("rejects invalid successful responses", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response("<!doctype html>")),
    );

    await expect(
      readSourceFile({ path: "src/app.ts", threadId: "thread-1" }),
    ).rejects.toEqual(
      new SourceFileReadError(
        "File API returned an invalid response.",
        200,
        null,
      ),
    );
  });
});
