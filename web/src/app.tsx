import { useEffect, useMemo, useState } from "react";
import {
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
  useParams,
} from "react-router";

import {
  archiveThread,
  getThreadListSnapshot,
  getThreadSnapshot,
  interruptTurn,
  renameThread,
  refreshThreadList,
  respondToServerRequest,
  sendPrompt,
  setForegroundThreadId,
  subscribeThread,
  subscribeThreadList,
  type ThreadDetailsSnapshot,
  type ThreadListSnapshot,
} from "@/lib/backend-store";
import { requestIdKey } from "@/lib/request-id";
import { activeTurn } from "@/lib/thread-state";
import type { AnyServerRequest } from "@/types/protocol";

import { ThreadList } from "./components/thread-list";
import { ThreadView } from "./components/thread-view";

const NOTIFICATION_CLICK_MESSAGE_TYPE = "codex-notification-click";

export function App() {
  return (
    <Routes>
      <Route path="/" element={<AppRoute />} />
      <Route path="/threads/:threadId" element={<AppRoute />} />
      <Route path="*" element={<Navigate replace to="/" />} />
    </Routes>
  );
}

function AppRoute() {
  const { threadId } = useParams();
  const location = useLocation();
  const navigate = useNavigate();
  const selectedThreadId = threadId ?? null;
  const [listState, setListState] = useState<ThreadListSnapshot>(
    getThreadListSnapshot,
  );
  const [threadState, setThreadState] = useState<ThreadDetailsSnapshot | null>(
    null,
  );
  const [archivingThread, setArchivingThread] = useState(false);
  const [sendingPrompt, setSendingPrompt] = useState(false);
  const [renamingThread, setRenamingThread] = useState(false);
  const [respondingRequestIds, setRespondingRequestIds] = useState<
    ReadonlySet<string>
  >(() => new Set());

  useEffect(() => subscribeThreadList(setListState), []);

  useEffect(() => {
    if (!selectedThreadId) {
      setThreadState(null);
      return;
    }

    setThreadState(getThreadSnapshot(selectedThreadId));
    return subscribeThread(selectedThreadId, setThreadState);
  }, [selectedThreadId]);

  useEffect(() => {
    const reportForegroundThread = () => {
      void setForegroundThreadId(
        document.visibilityState === "visible" ? selectedThreadId : null,
      );
    };

    reportForegroundThread();
    document.addEventListener("visibilitychange", reportForegroundThread);
    return () => {
      document.removeEventListener("visibilitychange", reportForegroundThread);
      void setForegroundThreadId(null);
    };
  }, [selectedThreadId]);

  useEffect(() => {
    if (!("serviceWorker" in navigator)) {
      return;
    }

    const handleServiceWorkerMessage = (event: MessageEvent) => {
      const path = notificationClickPath(event.data);
      if (!path) {
        return;
      }

      navigate(path);
    };

    navigator.serviceWorker.addEventListener(
      "message",
      handleServiceWorkerMessage,
    );
    return () => {
      navigator.serviceWorker.removeEventListener(
        "message",
        handleServiceWorkerMessage,
      );
    };
  }, [navigate]);

  const thread = threadState?.thread ?? null;
  const pendingRequests = threadState?.pendingRequests ?? [];
  const active = useMemo(() => activeTurn(thread), [thread]);
  const threadWarnings = [
    ...listState.warnings,
    ...(threadState?.warnings ?? []),
  ];

  useEffect(() => {
    const pendingRequestIds = new Set(
      pendingRequests.map((request) => requestIdKey(request.id)),
    );
    setRespondingRequestIds((current) => {
      const next = new Set<string>();
      for (const requestId of current) {
        if (pendingRequestIds.has(requestId)) {
          next.add(requestId);
        }
      }
      return next.size === current.size ? current : next;
    });
  }, [pendingRequests]);

  const handleSelectThread = (threadId: string) => {
    navigate(`/threads/${encodeURIComponent(threadId)}`, {
      state: { fromThreadList: true },
    });
  };

  const handleBackToThreads = () => {
    const state = location.state as { fromThreadList?: unknown } | null;
    if (state?.fromThreadList === true) {
      navigate(-1);
      return;
    }

    navigate("/", { replace: true });
  };

  const handleSendPrompt = async (text: string) => {
    if (!selectedThreadId) {
      return;
    }

    setSendingPrompt(true);
    try {
      await sendPrompt(selectedThreadId, text);
    } finally {
      setSendingPrompt(false);
    }
  };

  const handleInterrupt = async () => {
    if (!selectedThreadId || !active) {
      return;
    }

    await interruptTurn(selectedThreadId, active.id);
  };

  const handleRenameThread = async (name: string) => {
    if (!selectedThreadId) {
      return;
    }

    setRenamingThread(true);
    try {
      await renameThread(selectedThreadId, name);
    } finally {
      setRenamingThread(false);
    }
  };

  const handleArchiveThread = async () => {
    if (!selectedThreadId) {
      return;
    }

    setArchivingThread(true);
    try {
      await archiveThread(selectedThreadId);
      navigate("/", { replace: true });
    } finally {
      setArchivingThread(false);
    }
  };

  const handleRespondToRequest = (
    request: AnyServerRequest,
    response: unknown,
  ) => {
    const requestId = requestIdKey(request.id);
    setRespondingRequestIds((current) => {
      if (current.has(requestId)) {
        return current;
      }
      const next = new Set(current);
      next.add(requestId);
      return next;
    });

    void respondToServerRequest(request.id, response).catch(
      (error: unknown) => {
        setRespondingRequestIds((current) => {
          if (!current.has(requestId)) {
            return current;
          }
          const next = new Set(current);
          next.delete(requestId);
          return next;
        });
        console.error("Failed to respond to server request.", error);
      },
    );
  };

  return (
    <main
      className={selectedThreadId ? "app-shell app-shell-thread" : "app-shell"}
    >
      {!selectedThreadId ? (
        <>
          {listState.warnings.length > 0 && (
            <section className="warning-section list-warning-section">
              {listState.warnings.map((warning) => (
                <p key={warning} className="warning-banner">
                  {warning}
                </p>
              ))}
            </section>
          )}
          <ThreadList
            loading={listState.loading}
            onRefresh={() => void refreshThreadList()}
            onSelect={handleSelectThread}
            previewsByThreadId={listState.previewsByThreadId}
            threadActivityByThreadId={listState.threadActivityByThreadId}
            threads={listState.threads}
          />
        </>
      ) : (
        <ThreadView
          connectionError={listState.connectionError}
          connectionState={listState.connectionState}
          initializeSummary={listState.initializeSummary}
          interruptDisabled={!active}
          loading={threadState?.loading ?? true}
          archiving={archivingThread}
          onArchiveThread={handleArchiveThread}
          onBack={handleBackToThreads}
          onInterrupt={() => void handleInterrupt()}
          onRenameThread={handleRenameThread}
          onRespondToRequest={handleRespondToRequest}
          onSendPrompt={handleSendPrompt}
          pendingRequests={pendingRequests}
          respondingRequestIds={respondingRequestIds}
          runtimeText={threadState?.itemRuntimeText ?? {}}
          renaming={renamingThread}
          sending={sendingPrompt}
          thread={thread}
          warnings={threadWarnings}
        />
      )}
    </main>
  );
}

function notificationClickPath(data: unknown): string | null {
  if (
    !data ||
    typeof data !== "object" ||
    !("type" in data) ||
    data.type !== NOTIFICATION_CLICK_MESSAGE_TYPE ||
    !("url" in data) ||
    typeof data.url !== "string"
  ) {
    return null;
  }

  try {
    const url = new URL(data.url, window.location.origin);
    return url.origin === window.location.origin
      ? `${url.pathname}${url.search}${url.hash}`
      : null;
  } catch {
    return null;
  }
}
