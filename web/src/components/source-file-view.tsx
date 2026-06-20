import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";

import { readSourceFile, SourceFileReadError } from "@/lib/source-files";
import type { SourceFileReadResponse } from "@/lib/source-file-protocol";

import { MarkdownBlock } from "./markdown-block";

type Props = {
  onBack: () => void;
  onOpenSourceFile: (route: string) => void;
  path: string | null;
  threadId: string | null;
};

type LoadState =
  | { type: "error"; message: string }
  | { type: "loaded"; file: SourceFileReadResponse }
  | { type: "loading" };

function longestBacktickRun(text: string): number {
  let longest = 0;
  for (const match of text.matchAll(/`+/gu)) {
    longest = Math.max(longest, match[0].length);
  }
  return longest;
}

function fencedCodeMarkdown(content: string, language: string): string {
  const fence = "`".repeat(Math.max(3, longestBacktickRun(content) + 1));
  return `${fence}${language}\n${content}\n${fence}`;
}

function fileMarkdown(file: SourceFileReadResponse): string {
  if (file.fileKind === "markdown") {
    return file.content;
  }

  return fencedCodeMarkdown(file.content, file.language);
}

function formatBytes(value: number): string {
  if (value < 1024) {
    return `${value} B`;
  }

  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }

  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

function ButtonIcon({ children }: { children: ReactNode }) {
  return (
    <svg
      aria-hidden="true"
      className="button-icon"
      fill="none"
      focusable="false"
      viewBox="0 0 24 24"
    >
      {children}
    </svg>
  );
}

export function SourceFileView({
  onBack,
  onOpenSourceFile,
  path,
  threadId,
}: Props) {
  const [state, setState] = useState<LoadState>({ type: "loading" });
  const contentRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!threadId || !path) {
      setState({ type: "error", message: "Missing file route parameters." });
      return;
    }

    const controller = new AbortController();
    setState({ type: "loading" });
    void readSourceFile({
      path,
      signal: controller.signal,
      threadId,
    })
      .then((file) => {
        setState({ type: "loaded", file });
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) {
          return;
        }

        setState({
          type: "error",
          message:
            error instanceof SourceFileReadError || error instanceof Error
              ? error.message
              : "Failed to read source file.",
        });
      });

    return () => controller.abort();
  }, [path, threadId]);

  const renderedMarkdown = useMemo(
    () => (state.type === "loaded" ? fileMarkdown(state.file) : ""),
    [state],
  );

  useEffect(() => {
    if (state.type !== "loaded" || state.file.line === null) {
      return;
    }

    const line = state.file.line;
    window.requestAnimationFrame(() => {
      const lines = contentRef.current?.querySelectorAll(
        '[data-streamdown="code-block-body"] code > span',
      );
      const target = lines?.item(line - 1);
      if (!target) {
        return;
      }

      contentRef.current
        ?.querySelector(".source-line-highlight")
        ?.classList.remove("source-line-highlight");
      target.classList.add("source-line-highlight");
      target.scrollIntoView({ block: "center" });
    });
  }, [state]);

  const title =
    state.type === "loaded" ? state.file.displayPath : (path ?? "Source file");

  return (
    <section className="thread-shell source-shell">
      <div className="thread-header">
        <button
          aria-label="Back"
          className="icon-button"
          type="button"
          onClick={onBack}
        >
          <ButtonIcon>
            <path d="M15.5 5 8.5 12l7 7" />
            <path d="M9 12h10" />
          </ButtonIcon>
        </button>
        <h1 title={title}>{title}</h1>
      </div>

      {state.type === "loading" && (
        <p className="loading-copy">Loading source file...</p>
      )}

      {state.type === "error" && (
        <p className="warning-banner">{state.message}</p>
      )}

      {state.type === "loaded" && (
        <>
          <p className="source-file-meta">
            {state.file.fileKind}
            {" · "}
            {state.file.language}
            {" · "}
            {formatBytes(state.file.sizeBytes)}
            {state.file.line !== null ? ` · line ${state.file.line}` : ""}
          </p>
          <div className="source-file-content" ref={contentRef}>
            <MarkdownBlock
              sourceFileLinks={{
                onNavigate: onOpenSourceFile,
                root: state.file.root,
                threadId: threadId ?? "",
              }}
              text={renderedMarkdown}
            />
          </div>
        </>
      )}
    </section>
  );
}
