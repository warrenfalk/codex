import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import {
  BottomAnchoredList,
  type BottomAnchoredListHandle,
  type BottomAnchoredListPosition,
} from "react-bottom-anchored-list";

import type { ConnectionViewState } from "@/lib/backend-store";
import { syncTextareaHeight } from "@/lib/textarea-autosize";
import type { AnyServerRequest, Thread, Turn } from "@/types/protocol";

import { ThreadItemView } from "./thread-item-view";
import { RequestCard } from "./request-cards";

type Props = {
  connectionError: string | null;
  connectionState: ConnectionViewState;
  initializeSummary: string | null;
  thread: Thread | null;
  pendingRequests: AnyServerRequest[];
  runtimeText: Record<string, string>;
  warnings: string[];
  onBack: () => void;
  onInterrupt: () => void;
  onRenameThread: (name: string) => Promise<void>;
  onRespondToRequest: (request: AnyServerRequest, response: unknown) => void;
  onSendPrompt: (text: string) => Promise<void>;
  sending: boolean;
  renaming: boolean;
  interruptDisabled: boolean;
  loading: boolean;
};

function getTurnKey(turn: Turn): string {
  return turn.id;
}

type UserMessageLocation = {
  itemId: string;
  turnIndex: number;
};

function userMessageAnchorId(itemId: string): string {
  return `user-message-anchor-${itemId}`;
}

function connectionStatusClass(connectionState: ConnectionViewState): string {
  return connectionState === "connected"
    ? "composer-status-connected"
    : "composer-status-disconnected";
}

function threadTitle(thread: Thread): string {
  return thread.name ?? (thread.preview || "Thread");
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

export function ThreadView({
  connectionError,
  connectionState,
  initializeSummary,
  thread,
  pendingRequests,
  runtimeText,
  warnings,
  onBack,
  onInterrupt,
  onRenameThread,
  onRespondToRequest,
  onSendPrompt,
  sending,
  renaming,
  interruptDisabled,
  loading,
}: Props) {
  const [prompt, setPrompt] = useState("");
  const [renameDraft, setRenameDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [editingThreadTitle, setEditingThreadTitle] = useState(false);
  const [isThreadAnchoredToEnd, setIsThreadAnchoredToEnd] = useState(true);
  const promptInputRef = useRef<HTMLTextAreaElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const turnListRef = useRef<BottomAnchoredListHandle>(null);
  const trimmedPrompt = prompt.trim();
  const trimmedRenameDraft = renameDraft.trim();
  const canInterrupt = !interruptDisabled;
  const interruptMode = trimmedPrompt.length === 0 && canInterrupt;
  const statusDetail =
    initializeSummary ?? connectionError ?? "version unknown";
  const hasPendingRequests = pendingRequests.length > 0;
  const previousUserMessageLocationsByItemId = thread
    ? (() => {
        const previousLocationsByItemId = new Map<
          string,
          UserMessageLocation
        >();
        let previousLocation: UserMessageLocation | null = null;
        for (const [turnIndex, turn] of thread.turns.entries()) {
          for (const item of turn.items) {
            if (item.type !== "userMessage") {
              continue;
            }
            if (previousLocation) {
              previousLocationsByItemId.set(item.id, previousLocation);
            }
            previousLocation = {
              itemId: item.id,
              turnIndex,
            };
          }
        }
        return previousLocationsByItemId;
      })()
    : new Map<string, UserMessageLocation>();

  useLayoutEffect(() => {
    const input = promptInputRef.current;
    if (!input) {
      return;
    }

    syncTextareaHeight(input);
  }, [prompt]);

  useLayoutEffect(() => {
    if (!editingThreadTitle) {
      return;
    }

    renameInputRef.current?.focus();
    renameInputRef.current?.select();
  }, [editingThreadTitle]);

  useLayoutEffect(() => {
    setIsThreadAnchoredToEnd(true);
    setEditingThreadTitle(false);
    setRenameDraft("");
    setRenameError(null);
  }, [thread?.id]);

  const handleThreadPositionChange = (position: BottomAnchoredListPosition) => {
    setIsThreadAnchoredToEnd((current) =>
      current === position.anchoredToEnd ? current : position.anchoredToEnd,
    );
  };

  const scrollThreadToBottom = () => {
    turnListRef.current?.scrollToEnd({ behavior: "smooth" });
  };

  const scrollThreadToBottomAfterLayout = () => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        scrollThreadToBottom();
      });
    });
  };

  const scrollToPreviousUserTurn = ({
    itemId,
    turnIndex,
  }: UserMessageLocation) => {
    turnListRef.current?.scrollToItem(turnIndex);
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        document.getElementById(userMessageAnchorId(itemId))?.scrollIntoView({
          behavior: "smooth",
          block: "start",
        });
      });
    });
  };

  const submitPrompt = async () => {
    if (!trimmedPrompt || sending) {
      return;
    }

    await onSendPrompt(trimmedPrompt);
    setPrompt("");
    scrollThreadToBottomAfterLayout();
  };

  const startThreadRename = () => {
    if (!thread) {
      return;
    }

    setRenameDraft(threadTitle(thread));
    setRenameError(null);
    setEditingThreadTitle(true);
  };

  const cancelThreadRename = () => {
    if (renaming) {
      return;
    }

    setEditingThreadTitle(false);
    setRenameDraft("");
    setRenameError(null);
  };

  const submitThreadRename = async () => {
    if (!trimmedRenameDraft || renaming) {
      return;
    }

    setRenameError(null);
    try {
      await onRenameThread(trimmedRenameDraft);
      setEditingThreadTitle(false);
      setRenameDraft("");
    } catch (error) {
      setRenameError(
        error instanceof Error ? error.message : "Failed to rename thread",
      );
    }
  };

  if (!thread) {
    return (
      <section className="thread-shell">
        <div className="thread-header">
          <button
            aria-label="Back to threads"
            className="icon-button"
            type="button"
            onClick={onBack}
          >
            <ButtonIcon>
              <path d="M15.5 5 8.5 12l7 7" />
              <path d="M9 12h10" />
            </ButtonIcon>
          </button>
          <h1>Loading thread…</h1>
        </div>
        {loading && <p className="loading-copy">Loading thread details…</p>}
      </section>
    );
  }

  return (
    <section className="thread-shell">
      <div className="thread-header">
        <button
          aria-label="Back to threads"
          className="icon-button"
          type="button"
          onClick={onBack}
        >
          <ButtonIcon>
            <path d="M15.5 5 8.5 12l7 7" />
            <path d="M9 12h10" />
          </ButtonIcon>
        </button>
        {editingThreadTitle ? (
          <form
            className="thread-title-form"
            onSubmit={(event) => {
              event.preventDefault();
              void submitThreadRename();
            }}
          >
            <input
              aria-label="Thread title"
              className="thread-title-input"
              ref={renameInputRef}
              value={renameDraft}
              onChange={(event) => setRenameDraft(event.target.value)}
            />
            <button
              aria-label="Save thread title"
              className="icon-button"
              disabled={!trimmedRenameDraft || renaming}
              title="Save thread title"
              type="submit"
            >
              <ButtonIcon>
                <path d="m5 12 4 4 10-10" />
              </ButtonIcon>
            </button>
            <button
              aria-label="Cancel thread rename"
              className="icon-button"
              disabled={renaming}
              title="Cancel thread rename"
              type="button"
              onClick={cancelThreadRename}
            >
              <ButtonIcon>
                <path d="M6 6l12 12" />
                <path d="M18 6 6 18" />
              </ButtonIcon>
            </button>
          </form>
        ) : (
          <>
            <h1>{threadTitle(thread)}</h1>
            <button
              aria-label="Rename thread"
              className="icon-button"
              title="Rename thread"
              type="button"
              onClick={startThreadRename}
            >
              <ButtonIcon>
                <path d="M12 20h9" />
                <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
              </ButtonIcon>
            </button>
          </>
        )}
      </div>
      {renameError && <p className="thread-title-error">{renameError}</p>}

      {warnings.length > 0 && (
        <section className="warning-section">
          {warnings.map((warning) => (
            <p key={warning} className="warning-banner">
              {warning}
            </p>
          ))}
        </section>
      )}

      <div className="turn-list-frame">
        <BottomAnchoredList
          ref={turnListRef}
          className="turn-list"
          contentClassName="turn-list-content"
          getItemKey={getTurnKey}
          itemClassName="turn-list-item"
          items={thread.turns}
          onPositionChange={handleThreadPositionChange}
          renderItem={(turn) => (
            <section key={turn.id} className="turn-card">
              <div className="turn-items">
                {turn.items.map((item) => (
                  <ThreadItemView
                    anchorId={
                      item.type === "userMessage"
                        ? userMessageAnchorId(item.id)
                        : undefined
                    }
                    item={item}
                    key={item.id}
                    onJumpToPreviousUserTurn={
                      item.type === "userMessage" &&
                      previousUserMessageLocationsByItemId.has(item.id)
                        ? () =>
                            scrollToPreviousUserTurn(
                              previousUserMessageLocationsByItemId.get(
                                item.id,
                              )!,
                            )
                        : undefined
                    }
                    runtimeText={runtimeText[item.id]}
                  />
                ))}
              </div>
            </section>
          )}
        />
        {thread.turns.length > 0 && !isThreadAnchoredToEnd && (
          <button
            aria-label="Scroll thread to bottom"
            className="scroll-to-bottom-button"
            title="Scroll to bottom"
            type="button"
            onClick={scrollThreadToBottom}
          >
            {"↓"}
          </button>
        )}
      </div>

      {hasPendingRequests && (
        <section className="request-section">
          <div className="section-header">
            <div>
              <p className="eyebrow">Server requests</p>
              <h2>Action required</h2>
            </div>
          </div>
          <div className="request-stack">
            {pendingRequests.map((request) => (
              <RequestCard
                key={`${request.method}-${request.id}`}
                onRespond={onRespondToRequest}
                request={request}
              />
            ))}
          </div>
        </section>
      )}

      {!hasPendingRequests && (
        <form
          className="composer"
          onSubmit={async (event) => {
            event.preventDefault();
            await submitPrompt();
          }}
        >
          <p className="composer-status">
            <span
              className={`composer-status-state ${connectionStatusClass(
                connectionState,
              )}`}
            >
              [{connectionState}]
            </span>
            <span className="composer-status-detail">{statusDetail}</span>
          </p>
          <div className="composer-row">
            <textarea
              aria-label="Prompt"
              className="composer-input"
              id="composer-input"
              placeholder="Ask Codex to continue working…"
              ref={promptInputRef}
              rows={1}
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              onKeyDown={(event) => {
                if (
                  event.ctrlKey &&
                  event.shiftKey &&
                  event.key === "ArrowDown"
                ) {
                  event.preventDefault();
                  scrollThreadToBottom();
                  return;
                }

                if (event.ctrlKey && event.key === "Enter") {
                  event.preventDefault();
                  void submitPrompt();
                }
              }}
            />
            <button
              aria-label={
                interruptMode ? "Interrupt current turn" : "Send prompt"
              }
              className="composer-button"
              disabled={interruptMode ? false : sending || !trimmedPrompt}
              type={interruptMode ? "button" : "submit"}
              onClick={
                interruptMode
                  ? () => {
                      onInterrupt();
                    }
                  : undefined
              }
            >
              {interruptMode ? (
                <ButtonIcon>
                  <rect height="9" rx="1.5" width="9" x="7.5" y="7.5" />
                </ButtonIcon>
              ) : (
                <ButtonIcon>
                  <path d="M12 5v14" />
                  <path d="m6.5 10.5 5.5-5.5 5.5 5.5" />
                </ButtonIcon>
              )}
            </button>
          </div>
        </form>
      )}
    </section>
  );
}
