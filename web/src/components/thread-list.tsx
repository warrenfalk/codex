import { useEffect, useMemo, useRef, useState } from "react";
import { Streamdown, type StreamdownProps } from "streamdown";

import type { Thread } from "@/types/protocol";
import type { ProxyThreadActivity } from "@/lib/proxy-protocol";

import { PushNotificationControl } from "./push-notification-control";

type Props = {
  threads: Thread[];
  previewsByThreadId: Record<string, string>;
  threadActivityByThreadId: Record<string, ProxyThreadActivity>;
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

function activityForThread(
  thread: Thread,
  activity: ProxyThreadActivity | undefined,
): ProxyThreadActivity {
  if (activity) {
    return activity;
  }

  return {
    lastAgentMessage: null,
    lastUserMessage: null,
    state: thread.status.type === "active" ? "working" : "ready",
  };
}

function activityStatusLabel(activity: ProxyThreadActivity): string {
  switch (activity.state) {
    case "virgin":
      return "virgin";
    case "working":
      return "working";
    case "ready":
      return "ready";
    default:
      return "unknown";
  }
}

function searchTextForThread(
  thread: Thread,
  latestPreview: string,
  activity: ProxyThreadActivity,
): string {
  const branch = thread.gitInfo?.branch;
  const originUrl = thread.gitInfo?.originUrl;
  const sha = thread.gitInfo?.sha;
  return [
    thread.id,
    threadTitle(thread),
    thread.preview,
    latestPreview,
    activity.lastUserMessage,
    activity.lastAgentMessage,
    activity.state,
    statusLabel(thread),
    thread.status.type === "active"
      ? thread.status.activeFlags.join(" ")
      : null,
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

function ThreadActionsMenu({ onRefresh }: { onRefresh: () => void }) {
  const [isOpen, setIsOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  const closeMenu = () => {
    setIsOpen(false);
  };

  const handleRefresh = () => {
    onRefresh();
    closeMenu();
  };

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !menuRef.current?.contains(event.target)
      ) {
        setIsOpen(false);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsOpen(false);
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen]);

  return (
    <div className="thread-actions-menu" ref={menuRef}>
      <button
        aria-expanded={isOpen}
        aria-haspopup="true"
        aria-label="Thread list actions"
        className="icon-button thread-actions-trigger"
        title="Thread list actions"
        type="button"
        onClick={() => setIsOpen((open) => !open)}
      >
        <svg
          aria-hidden="true"
          className="button-icon"
          fill="currentColor"
          focusable="false"
          viewBox="0 0 24 24"
        >
          <circle cx="12" cy="5" r="1.8" />
          <circle cx="12" cy="12" r="1.8" />
          <circle cx="12" cy="19" r="1.8" />
        </svg>
      </button>
      {isOpen && (
        <div className="thread-actions-popover">
          <PushNotificationControl onAction={closeMenu} />
          <button type="button" onClick={handleRefresh}>
            Refresh
          </button>
        </div>
      )}
    </div>
  );
}

export function ThreadList({
  threads,
  previewsByThreadId,
  threadActivityByThreadId,
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
      const activity = activityForThread(
        thread,
        threadActivityByThreadId[thread.id],
      );
      const searchText = searchTextForThread(thread, latestPreview, activity);
      return searchTerms.every((term) => searchText.includes(term));
    });
  }, [previewsByThreadId, searchTerms, threadActivityByThreadId, threads]);

  return (
    <section className="list-shell">
      <div className="section-header">
        <div>
          <p className="eyebrow">Live session list</p>
          <h1>Threads</h1>
        </div>
        <div className="list-actions">
          <ThreadActionsMenu onRefresh={onRefresh} />
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
          const activity = activityForThread(
            thread,
            threadActivityByThreadId[thread.id],
          );
          const fallbackPreview =
            previewsByThreadId[thread.id] ?? thread.preview;
          const hasActivityMessages =
            Boolean(activity.lastUserMessage) ||
            Boolean(activity.lastAgentMessage);
          const isAwaitingApproval =
            thread.status.type === "active" &&
            thread.status.activeFlags.includes("waitingOnApproval");
          const activityState = isAwaitingApproval
            ? "approval"
            : activity.state;
          const activityLabel = isAwaitingApproval
            ? "needs approval"
            : activityStatusLabel(activity);
          return (
            <button
              key={thread.id}
              className={
                isAwaitingApproval
                  ? "thread-row thread-row-awaiting-approval"
                  : "thread-row"
              }
              type="button"
              onClick={() => onSelect(thread.id)}
            >
              <div className="thread-row-top">
                <h2>{threadTitle(thread)}</h2>
                <span className={`status-pill status-thread-${activityState}`}>
                  {activityLabel}
                </span>
              </div>
              <div className="thread-preview">
                {hasActivityMessages ? (
                  <>
                    {activity.lastUserMessage && (
                      <div className="thread-preview-line">
                        <span className="thread-preview-label">You</span>
                        <Streamdown
                          className="thread-preview-markdown thread-preview-last-message"
                          dir="auto"
                          linkSafety={markdownLinkSafety}
                        >
                          {activity.lastUserMessage}
                        </Streamdown>
                      </div>
                    )}
                    {activity.lastAgentMessage && (
                      <div className="thread-preview-line">
                        <span className="thread-preview-label">Codex</span>
                        <Streamdown
                          className="thread-preview-markdown thread-preview-last-message"
                          dir="auto"
                          linkSafety={markdownLinkSafety}
                        >
                          {activity.lastAgentMessage}
                        </Streamdown>
                      </div>
                    )}
                  </>
                ) : (
                  <Streamdown
                    className="thread-preview-markdown"
                    dir="auto"
                    linkSafety={markdownLinkSafety}
                  >
                    {fallbackPreview || statusLabel(thread) || "No preview yet"}
                  </Streamdown>
                )}
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
