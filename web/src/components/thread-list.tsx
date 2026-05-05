import { useMemo, useState } from "react";
import { Streamdown, type StreamdownProps } from "streamdown";

import type { Thread } from "@/types/protocol";

import { PushNotificationControl } from "./push-notification-control";

type Props = {
  threads: Thread[];
  previewsByThreadId: Record<string, string>;
  loading: boolean;
  onRefresh: () => void;
  onSelect: (threadId: string) => void;
};

const markdownLinkSafety: NonNullable<StreamdownProps["linkSafety"]> = {
  enabled: false,
};

function threadTitle(thread: Thread): string {
  return thread.name ?? (thread.preview || "Untitled thread");
}

function statusLabel(thread: Thread): string {
  switch (thread.status.type) {
    case "active":
      return thread.status.activeFlags.length > 0
        ? `active: ${thread.status.activeFlags.join(", ")}`
        : "active";
    case "idle":
      return "idle";
    case "notLoaded":
      return "not loaded";
    case "systemError":
      return "system error";
    default:
      return "unknown";
  }
}

function searchTextForThread(thread: Thread, latestPreview: string): string {
  const branch = thread.gitInfo?.branch;
  const originUrl = thread.gitInfo?.originUrl;
  const sha = thread.gitInfo?.sha;
  return [
    thread.id,
    threadTitle(thread),
    thread.preview,
    latestPreview,
    thread.cwd,
    `cwd:${thread.cwd}`,
    branch,
    branch ? `branch:${branch}` : null,
    originUrl,
    sha,
    thread.path,
    thread.modelProvider,
    thread.cliVersion,
    thread.source,
    thread.agentNickname,
    thread.agentRole,
  ]
    .filter((part): part is string => typeof part === "string" && part !== "")
    .join("\n")
    .toLocaleLowerCase();
}

export function ThreadList({
  threads,
  previewsByThreadId,
  loading,
  onRefresh,
  onSelect,
}: Props) {
  const [searchQuery, setSearchQuery] = useState("");
  const searchTerms = useMemo(
    () => searchQuery.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean),
    [searchQuery],
  );
  const filteredThreads = useMemo(() => {
    if (searchTerms.length === 0) {
      return threads;
    }

    return threads.filter((thread) => {
      const latestPreview = previewsByThreadId[thread.id] ?? thread.preview;
      const searchText = searchTextForThread(thread, latestPreview);
      return searchTerms.every((term) => searchText.includes(term));
    });
  }, [previewsByThreadId, searchTerms, threads]);

  return (
    <section className="list-shell">
      <div className="section-header">
        <div>
          <p className="eyebrow">Live session list</p>
          <h1>Threads</h1>
        </div>
        <div className="list-actions">
          <PushNotificationControl />
          <button type="button" onClick={onRefresh}>
            Refresh
          </button>
        </div>
      </div>
      <div className="thread-search">
        <input
          aria-label="Search threads"
          placeholder="Search threads"
          type="search"
          value={searchQuery}
          onChange={(event) => setSearchQuery(event.target.value)}
        />
      </div>
      {loading && <p className="loading-copy">Loading threads…</p>}
      <div className="thread-list">
        {filteredThreads.map((thread) => {
          const branch = thread.gitInfo?.branch;
          return (
            <button
              key={thread.id}
              className="thread-row"
              type="button"
              onClick={() => onSelect(thread.id)}
            >
              <div className="thread-row-top">
                <h2>{threadTitle(thread)}</h2>
                <span className={`status-pill status-${thread.status.type}`}>
                  {statusLabel(thread)}
                </span>
              </div>
              <div className="thread-preview">
                <Streamdown
                  className="thread-preview-markdown"
                  dir="auto"
                  linkSafety={markdownLinkSafety}
                >
                  {(previewsByThreadId[thread.id] ?? thread.preview) ||
                    "No preview yet"}
                </Streamdown>
              </div>
              <div className="thread-row-meta">
                <span>{thread.cwd}</span>
                {branch && <span>{branch}</span>}
                <span>{thread.modelProvider}</span>
                <span>
                  {new Date(thread.updatedAt * 1000).toLocaleString()}
                </span>
              </div>
            </button>
          );
        })}
      </div>
      {!loading && filteredThreads.length === 0 && (
        <p className="empty-copy">No matching threads.</p>
      )}
    </section>
  );
}
