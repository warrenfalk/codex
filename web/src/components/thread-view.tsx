import {
  type ReactNode,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  BottomAnchoredList,
  type BottomAnchoredListHandle,
  type BottomAnchoredListPosition,
} from "react-bottom-anchored-list";

import type { ConnectionViewState } from "@/lib/backend-store";
import { requestIdKey } from "@/lib/request-id";
import { syncTextareaHeight } from "@/lib/textarea-autosize";
import type {
  AnyServerRequest,
  Thread,
  ThreadItem,
  Turn,
} from "@/types/protocol";

import { ThreadItemView } from "./thread-item-view";
import { RequestCard } from "./request-cards";

type Props = {
  connectionError: string | null;
  connectionState: ConnectionViewState;
  initializeSummary: string | null;
  thread: Thread | null;
  pendingRequests: AnyServerRequest[];
  respondingRequestIds: ReadonlySet<string>;
  runtimeText: Record<string, string>;
  warnings: string[];
  onArchiveThread: () => Promise<void>;
  onBack: () => void;
  onInterrupt: () => void;
  onOpenSourceFile: (route: string) => void;
  onRenameThread: (name: string) => Promise<void>;
  onRespondToRequest: (request: AnyServerRequest, response: unknown) => void;
  onSendPrompt: (text: string) => Promise<void>;
  sending: boolean;
  archiving: boolean;
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

type TextSelection = {
  start: number;
  end: number;
};

type FileChangeItem = Extract<ThreadItem, { type: "fileChange" }>;
type FileChange = FileChangeItem["changes"][number];
type FileChangeApprovalRequest = Extract<
  AnyServerRequest,
  { method: "item/fileChange/requestApproval" }
>;

function userMessageAnchorId(itemId: string): string {
  return `user-message-anchor-${itemId}`;
}

function isFileChangeApprovalRequest(
  request: AnyServerRequest,
): request is FileChangeApprovalRequest {
  return request.method === "item/fileChange/requestApproval";
}

function fileChangesForRequest(
  thread: Thread | null,
  request: AnyServerRequest,
): FileChange[] {
  if (!isFileChangeApprovalRequest(request)) {
    return [];
  }

  const turn = thread?.turns.find((turn) => turn.id === request.params.turnId);
  const item = turn?.items.find(
    (item): item is FileChangeItem =>
      item.id === request.params.itemId && item.type === "fileChange",
  );
  return item?.changes ?? [];
}

function shouldSubmitPromptOnEnter(
  value: string,
  selectionStart: number,
  selectionEnd: number,
): boolean {
  if (selectionStart !== selectionEnd || value.trim().length === 0) {
    return false;
  }

  const afterCursor = value.slice(selectionEnd);
  if (afterCursor.trim().length > 0) {
    return false;
  }

  const beforeCursor = value
    .slice(0, selectionStart)
    .replace(/[^\S\r\n]+$/u, "");
  return /(?:\r\n|\r|\n)[^\S\r\n]*(?:\r\n|\r|\n)$/u.test(beforeCursor);
}

function connectionStatusClass(connectionState: ConnectionViewState): string {
  return connectionState === "connected"
    ? "composer-status-connected"
    : "composer-status-disconnected";
}

function threadTitle(thread: Thread): string {
  return thread.name ?? (thread.preview || "Thread");
}

function isCondensableThreadItem(item: ThreadItem): boolean {
  switch (item.type) {
    case "userMessage":
    case "agentMessage":
      return false;
    case "hookPrompt":
    case "plan":
    case "reasoning":
    case "commandExecution":
    case "fileChange":
    case "mcpToolCall":
    case "dynamicToolCall":
    case "collabAgentToolCall":
    case "webSearch":
    case "imageView":
    case "imageGeneration":
    case "enteredReviewMode":
    case "exitedReviewMode":
    case "contextCompaction":
      return true;
  }

  const exhaustive: never = item;
  return exhaustive;
}

function turnsForCondensedMode(turns: Turn[], condensedMode: boolean): Turn[] {
  if (!condensedMode) {
    return turns;
  }

  let itemIndex = 0;
  let lastAgentMessageIndex = -1;
  for (const turn of turns) {
    for (const item of turn.items) {
      if (item.type === "agentMessage") {
        lastAgentMessageIndex = itemIndex;
      }
      itemIndex += 1;
    }
  }

  if (lastAgentMessageIndex === -1) {
    return turns;
  }

  itemIndex = 0;
  return turns.flatMap((turn) => {
    const items = turn.items.filter((item) => {
      const isVisible =
        !isCondensableThreadItem(item) || itemIndex > lastAgentMessageIndex;
      itemIndex += 1;
      return isVisible;
    });

    return items.length > 0 ? [{ ...turn, items }] : [];
  });
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

function ThreadActionsMenu({
  archiving,
  condensedMode,
  renaming,
  onArchive,
  onToggleCondensedMode,
  onRename,
}: {
  archiving: boolean;
  condensedMode: boolean;
  renaming: boolean;
  onArchive: () => void;
  onToggleCondensedMode: () => void;
  onRename: () => void;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

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

  const handleRename = () => {
    setIsOpen(false);
    onRename();
  };

  const handleArchive = () => {
    setIsOpen(false);
    onArchive();
  };

  return (
    <div className="thread-actions-menu" ref={menuRef}>
      <button
        aria-expanded={isOpen}
        aria-haspopup="true"
        aria-label="Thread actions"
        className="icon-button thread-actions-trigger"
        title="Thread actions"
        type="button"
        onClick={() => setIsOpen((open) => !open)}
      >
        <ButtonIcon>
          <circle cx="12" cy="5" fill="currentColor" r="1.8" stroke="none" />
          <circle cx="12" cy="12" fill="currentColor" r="1.8" stroke="none" />
          <circle cx="12" cy="19" fill="currentColor" r="1.8" stroke="none" />
        </ButtonIcon>
      </button>
      {isOpen && (
        <div className="thread-actions-popover">
          <button
            aria-pressed={condensedMode}
            type="button"
            onClick={onToggleCondensedMode}
          >
            Condensed
          </button>
          <button
            disabled={renaming || archiving}
            type="button"
            onClick={handleRename}
          >
            Rename
          </button>
          <button disabled={archiving} type="button" onClick={handleArchive}>
            {archiving ? "Archiving..." : "Archive"}
          </button>
        </div>
      )}
    </div>
  );
}

export function ThreadView({
  connectionError,
  connectionState,
  initializeSummary,
  thread,
  pendingRequests,
  respondingRequestIds,
  runtimeText,
  warnings,
  onArchiveThread,
  onBack,
  onInterrupt,
  onOpenSourceFile,
  onRenameThread,
  onRespondToRequest,
  onSendPrompt,
  sending,
  archiving,
  renaming,
  interruptDisabled,
  loading,
}: Props) {
  const [prompt, setPrompt] = useState("");
  const [promptSelection, setPromptSelection] = useState<TextSelection>({
    start: 0,
    end: 0,
  });
  const [renameDraft, setRenameDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [editingThreadTitle, setEditingThreadTitle] = useState(false);
  const [condensedMode, setCondensedMode] = useState(true);
  const [isThreadAnchoredToEnd, setIsThreadAnchoredToEnd] = useState(true);
  const promptInputRef = useRef<HTMLTextAreaElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const turnListRef = useRef<BottomAnchoredListHandle>(null);
  const promptHasContent = prompt.trim().length > 0;
  const promptToSend = prompt.trimEnd();
  const trimmedRenameDraft = renameDraft.trim();
  const canInterrupt = !interruptDisabled;
  const interruptMode = !promptHasContent && canInterrupt;
  const willSubmitPromptOnEnter =
    !sending &&
    shouldSubmitPromptOnEnter(
      prompt,
      promptSelection.start,
      promptSelection.end,
    );
  const statusDetail =
    connectionState === "connected"
      ? (thread?.cwd ?? "cwd unknown")
      : (connectionError ??
        thread?.cwd ??
        initializeSummary ??
        "connection unavailable");
  const hasPendingRequests = pendingRequests.length > 0;
  const visibleTurns = thread
    ? turnsForCondensedMode(thread.turns, condensedMode)
    : [];
  const previousUserMessageLocationsByItemId = thread
    ? (() => {
        const previousLocationsByItemId = new Map<
          string,
          UserMessageLocation
        >();
        let previousLocation: UserMessageLocation | null = null;
        for (const [turnIndex, turn] of visibleTurns.entries()) {
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

  const syncPromptSelection = (input: HTMLTextAreaElement) => {
    const nextSelection = {
      start: input.selectionStart,
      end: input.selectionEnd,
    };

    setPromptSelection((current) =>
      current.start === nextSelection.start && current.end === nextSelection.end
        ? current
        : nextSelection,
    );
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
    if (!promptHasContent || sending) {
      return;
    }

    await onSendPrompt(promptToSend);
    setPrompt("");
    setPromptSelection({ start: 0, end: 0 });
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

  const archiveThread = async () => {
    setRenameError(null);
    try {
      await onArchiveThread();
    } catch (error) {
      setRenameError(
        error instanceof Error ? error.message : "Failed to archive thread",
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
            <ThreadActionsMenu
              archiving={archiving}
              condensedMode={condensedMode}
              renaming={renaming}
              onArchive={() => void archiveThread()}
              onToggleCondensedMode={() => setCondensedMode((mode) => !mode)}
              onRename={startThreadRename}
            />
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
          items={visibleTurns}
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
                    sourceFileLinks={{
                      onNavigate: onOpenSourceFile,
                      root: thread.cwd,
                      threadId: thread.id,
                    }}
                    runtimeText={runtimeText[item.id]}
                  />
                ))}
              </div>
            </section>
          )}
        />
        {visibleTurns.length > 0 && !isThreadAnchoredToEnd && (
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
                fileChanges={fileChangesForRequest(thread, request)}
                key={`${request.method}-${request.id}`}
                onRespond={onRespondToRequest}
                request={request}
                responding={respondingRequestIds.has(requestIdKey(request.id))}
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
              onChange={(event) => {
                setPrompt(event.currentTarget.value);
                syncPromptSelection(event.currentTarget);
              }}
              onFocus={(event) => syncPromptSelection(event.currentTarget)}
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
                  return;
                }

                if (
                  !sending &&
                  event.key === "Enter" &&
                  shouldSubmitPromptOnEnter(
                    event.currentTarget.value,
                    event.currentTarget.selectionStart,
                    event.currentTarget.selectionEnd,
                  )
                ) {
                  event.preventDefault();
                  void submitPrompt();
                }
              }}
              onKeyUp={(event) => syncPromptSelection(event.currentTarget)}
              onSelect={(event) => syncPromptSelection(event.currentTarget)}
            />
            <button
              aria-label={
                interruptMode ? "Interrupt current turn" : "Send prompt"
              }
              className={`composer-button${
                willSubmitPromptOnEnter ? " composer-button-enter-submit" : ""
              }`}
              disabled={interruptMode ? false : sending || !promptHasContent}
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
