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
import { activeTurn } from "@/lib/thread-state";
import type { AnyServerRequest } from "@/types/protocol";

import { ThreadList } from "./components/thread-list";
import { ThreadView } from "./components/thread-view";

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
  const [sendingPrompt, setSendingPrompt] = useState(false);
  const [renamingThread, setRenamingThread] = useState(false);

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

  const thread = threadState?.thread ?? null;
  const active = useMemo(() => activeTurn(thread), [thread]);
  const threadWarnings = [
    ...listState.warnings,
    ...(threadState?.warnings ?? []),
  ];

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

  const handleRespondToRequest = async (
    request: AnyServerRequest,
    response: unknown,
  ) => {
    await respondToServerRequest(request.id, response);
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
          onBack={handleBackToThreads}
          onInterrupt={() => void handleInterrupt()}
          onRenameThread={handleRenameThread}
          onRespondToRequest={(request, response) => {
            void handleRespondToRequest(request, response);
          }}
          onSendPrompt={handleSendPrompt}
          pendingRequests={threadState?.pendingRequests ?? []}
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
