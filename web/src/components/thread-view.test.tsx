import type { ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AnyServerRequest, Thread, Turn } from "@/types/protocol";

import { ThreadView } from "./thread-view";

afterEach(() => {
  cleanup();
});

const scrollToEndMock = vi.hoisted(() => vi.fn());
const scrollToItemMock = vi.hoisted(() => vi.fn());
const scrollIntoViewMock = vi.hoisted(() => vi.fn());

beforeEach(() => {
  scrollIntoViewMock.mockReset();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: scrollIntoViewMock,
  });
});

vi.mock("react-bottom-anchored-list", async () => {
  const React = await import("react");

  return {
    BottomAnchoredList: React.forwardRef(function MockBottomAnchoredList(
      {
        className,
        items,
        onPositionChange,
        renderItem,
      }: {
        className?: string;
        items: Turn[];
        onPositionChange?: (position: {
          anchoredToEnd: boolean;
          renderedLowerIndex: number;
          tailIndex: number;
          scrollTop: number;
          scrollHeight: number;
          clientHeight: number;
        }) => void;
        renderItem: (item: Turn) => ReactNode;
      },
      ref: React.ForwardedRef<{
        scrollToEnd: typeof scrollToEndMock;
        scrollToItem: typeof scrollToItemMock;
      }>,
    ) {
      React.useImperativeHandle(ref, () => ({
        scrollToEnd: scrollToEndMock,
        scrollToItem: scrollToItemMock,
      }));

      return (
        <div className={className} data-testid="turn-list">
          {items.map(renderItem)}
          <button
            type="button"
            onClick={() =>
              onPositionChange?.({
                anchoredToEnd: false,
                clientHeight: 100,
                renderedLowerIndex: 0,
                scrollHeight: 500,
                scrollTop: 0,
                tailIndex: items.length - 1,
              })
            }
          >
            Mark thread scrolled backward
          </button>
          <button
            type="button"
            onClick={() =>
              onPositionChange?.({
                anchoredToEnd: true,
                clientHeight: 100,
                renderedLowerIndex: 0,
                scrollHeight: 500,
                scrollTop: 400,
                tailIndex: items.length - 1,
              })
            }
          >
            Mark thread anchored to bottom
          </button>
        </div>
      );
    }),
  };
});

function buildThread(turns: Turn[] = []): Thread {
  return {
    id: "thread-1",
    sessionId: "thread-1",
    forkedFromId: null,
    preview: "Investigate prompt controls",
    ephemeral: false,
    modelProvider: "openai",
    createdAt: 1,
    updatedAt: 2,
    status: { type: "idle" },
    path: null,
    cwd: "/workspace",
    cliVersion: "0.0.0",
    source: "appServer",
    threadSource: null,
    agentNickname: null,
    agentRole: null,
    gitInfo: null,
    name: "Thread name",
    turns,
  };
}

function buildTurn(id = "turn-1"): Turn {
  return {
    id,
    items: [
      {
        type: "agentMessage",
        id: `${id}-agent`,
        text: "Working on it.",
        phase: null,
        memoryCitation: null,
      },
    ],
    itemsView: "full",
    status: "completed",
    error: null,
    startedAt: null,
    completedAt: null,
    durationMs: null,
  };
}

function buildUserTurn(id: string, text: string): Turn {
  return {
    id,
    items: [
      {
        type: "userMessage",
        id: `${id}-user`,
        content: [
          {
            type: "text",
            text,
            text_elements: [],
          },
        ],
      },
    ],
    itemsView: "full",
    status: "completed",
    error: null,
    startedAt: null,
    completedAt: null,
    durationMs: null,
  };
}

function renderThreadView(
  overrides: Partial<Parameters<typeof ThreadView>[0]> = {},
) {
  const props: Parameters<typeof ThreadView>[0] = {
    connectionError: null,
    connectionState: "connected",
    initializeSummary: "codex 0.0.0 on linux",
    interruptDisabled: true,
    loading: false,
    onBack: vi.fn(),
    onInterrupt: vi.fn(),
    onRenameThread: vi.fn().mockResolvedValue(undefined),
    onRespondToRequest: vi.fn(),
    onSendPrompt: vi.fn().mockResolvedValue(undefined),
    pendingRequests: [],
    renaming: false,
    respondingRequestIds: new Set(),
    runtimeText: {},
    sending: false,
    thread: buildThread(),
    warnings: [],
    ...overrides,
  };

  const view = render(<ThreadView {...props} />);
  return { ...view, props };
}

function changePrompt(
  value: string,
  selectionStart = value.length,
  selectionEnd = selectionStart,
): HTMLTextAreaElement {
  const prompt = screen.getByLabelText("Prompt") as HTMLTextAreaElement;
  fireEvent.change(prompt, { target: { value } });
  prompt.setSelectionRange(selectionStart, selectionEnd);
  fireEvent.select(prompt);
  return prompt;
}

describe("ThreadView", () => {
  it("hides the composer during approval prompts and preserves the draft", () => {
    const pendingRequest: AnyServerRequest = {
      method: "item/commandExecution/requestApproval",
      id: 7,
      params: {
        threadId: "thread-1",
        turnId: "turn-1",
        itemId: "item-1",
        startedAtMs: 1,
        command: "printf test",
      },
    };
    const view = renderThreadView();

    fireEvent.change(screen.getByLabelText("Prompt"), {
      target: { value: "draft while approval is pending" },
    });

    view.rerender(
      <ThreadView {...view.props} pendingRequests={[pendingRequest]} />,
    );

    expect(screen.getByText("Action required")).toBeInTheDocument();
    expect(screen.queryByLabelText("Prompt")).not.toBeInTheDocument();

    view.rerender(<ThreadView {...view.props} pendingRequests={[]} />);

    expect(screen.getByLabelText("Prompt")).toHaveValue(
      "draft while approval is pending",
    );
  });

  it("shows file paths for file change approval prompts", () => {
    const pendingRequest: AnyServerRequest = {
      method: "item/fileChange/requestApproval",
      id: 8,
      params: {
        threadId: "thread-1",
        turnId: "turn-1",
        itemId: "patch-1",
        startedAtMs: 1,
        reason: "Need to edit files",
        grantRoot: null,
      },
    };
    renderThreadView({
      pendingRequests: [pendingRequest],
      thread: buildThread([
        {
          id: "turn-1",
          items: [
            {
              type: "fileChange",
              id: "patch-1",
              status: "inProgress",
              changes: [
                {
                  path: "src/components/thread-view.tsx",
                  kind: { type: "update", move_path: null },
                  diff: "@@ -1 +1 @@",
                },
              ],
            },
          ],
          itemsView: "full",
          status: "inProgress",
          error: null,
          startedAt: null,
          completedAt: null,
          durationMs: null,
        },
      ]),
    });

    expect(screen.getByText("Need to edit files")).toBeInTheDocument();
    expect(
      screen
        .getAllByText("src/components/thread-view.tsx")
        .some((element) => element.classList.contains("mono")),
    ).toBe(true);
  });

  it("submits the prompt with Ctrl+Enter", async () => {
    scrollToEndMock.mockReset();
    scrollToItemMock.mockReset();
    const onSendPrompt = vi.fn().mockResolvedValue(undefined);
    renderThreadView({ onSendPrompt });

    const prompt = screen.getByLabelText("Prompt");
    fireEvent.change(prompt, { target: { value: "continue working" } });
    fireEvent.keyDown(prompt, { ctrlKey: true, key: "Enter" });

    await waitFor(() => {
      expect(onSendPrompt).toHaveBeenCalledWith("continue working");
    });
    expect(prompt).toHaveValue("");
    await waitFor(() => {
      expect(scrollToEndMock).toHaveBeenCalledWith({
        behavior: "smooth",
      });
    });
  });

  it("submits the prompt instead of adding a third trailing line ending", async () => {
    const onSendPrompt = vi.fn().mockResolvedValue(undefined);
    renderThreadView({ onSendPrompt });

    const cursorIndex = "continue working\n\n".length;
    const prompt = changePrompt("continue working\n\n   ", cursorIndex);

    expect(screen.getByRole("button", { name: "Send prompt" })).toHaveClass(
      "composer-button-enter-submit",
    );

    fireEvent.keyDown(prompt, { key: "Enter" });

    await waitFor(() => {
      expect(onSendPrompt).toHaveBeenCalledWith("continue working");
    });
    expect(prompt).toHaveValue("");
  });

  it("keeps Enter as a newline before the third trailing line ending", () => {
    const onSendPrompt = vi.fn().mockResolvedValue(undefined);
    const { rerender, props } = renderThreadView({ onSendPrompt });

    const prompt = changePrompt("continue working\n");

    expect(screen.getByRole("button", { name: "Send prompt" })).not.toHaveClass(
      "composer-button-enter-submit",
    );

    fireEvent.keyDown(prompt, { key: "Enter" });

    expect(onSendPrompt).not.toHaveBeenCalled();

    const cursorBeforeText = "continue working\n\n".length;
    changePrompt("continue working\n\nmore context", cursorBeforeText);

    expect(screen.getByRole("button", { name: "Send prompt" })).not.toHaveClass(
      "composer-button-enter-submit",
    );

    fireEvent.keyDown(prompt, { key: "Enter" });

    expect(onSendPrompt).not.toHaveBeenCalled();

    rerender(<ThreadView {...props} sending />);
    changePrompt("continue working\n\n");

    expect(screen.getByRole("button", { name: "Send prompt" })).not.toHaveClass(
      "composer-button-enter-submit",
    );
  });

  it("scrolls the thread output to the bottom with Ctrl+Shift+Down", () => {
    scrollToEndMock.mockReset();
    scrollToItemMock.mockReset();
    renderThreadView({ thread: buildThread([buildTurn()]) });

    fireEvent.keyDown(screen.getByLabelText("Prompt"), {
      ctrlKey: true,
      key: "ArrowDown",
      shiftKey: true,
    });

    expect(scrollToEndMock).toHaveBeenCalledWith({
      behavior: "smooth",
    });
  });

  it("uses the composer action as interrupt when the prompt is empty", () => {
    scrollToEndMock.mockReset();
    scrollToItemMock.mockReset();
    const onInterrupt = vi.fn();
    renderThreadView({ interruptDisabled: false, onInterrupt });

    expect(screen.queryByText("Interrupt")).not.toBeInTheDocument();
    expect(
      screen
        .getByRole("button", { name: "Back to threads" })
        .querySelector(".button-icon"),
    ).not.toBeNull();
    expect(
      screen
        .getByRole("button", { name: "Interrupt current turn" })
        .querySelector(".button-icon"),
    ).not.toBeNull();

    fireEvent.click(
      screen.getByRole("button", { name: "Interrupt current turn" }),
    );

    expect(onInterrupt).toHaveBeenCalledTimes(1);
  });

  it("scrolls the thread output to the bottom when scrolled backward", async () => {
    scrollToEndMock.mockReset();
    scrollToItemMock.mockReset();
    renderThreadView({ thread: buildThread([buildTurn()]) });

    expect(
      screen.queryByRole("button", { name: "Scroll thread to bottom" }),
    ).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: "Mark thread scrolled backward",
      }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Scroll thread to bottom" }),
    );

    expect(scrollToEndMock).toHaveBeenCalledWith({
      behavior: "smooth",
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "Mark thread anchored to bottom",
      }),
    );

    expect(
      screen.queryByRole("button", { name: "Scroll thread to bottom" }),
    ).not.toBeInTheDocument();
  });

  it("uses an icon for the send action", () => {
    renderThreadView();

    fireEvent.change(screen.getByLabelText("Prompt"), {
      target: { value: "continue" },
    });

    expect(
      screen
        .getByRole("button", { name: "Send prompt" })
        .querySelector(".button-icon"),
    ).not.toBeNull();
  });

  it("submits thread renames without changing the local title", async () => {
    const onRenameThread = vi.fn().mockResolvedValue(undefined);
    renderThreadView({ onRenameThread });

    fireEvent.click(screen.getByRole("button", { name: "Rename thread" }));

    const titleInput = screen.getByRole("textbox", { name: "Thread title" });
    expect(titleInput).toHaveValue("Thread name");

    fireEvent.change(titleInput, { target: { value: "New thread title" } });
    fireEvent.click(screen.getByRole("button", { name: "Save thread title" }));

    await waitFor(() => {
      expect(onRenameThread).toHaveBeenCalledWith("New thread title");
    });
    expect(screen.queryByRole("textbox", { name: "Thread title" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Thread name" }),
    ).toBeInTheDocument();
  });

  it("jumps to the previous user turn from user messages", async () => {
    scrollToEndMock.mockReset();
    scrollToItemMock.mockReset();
    renderThreadView({
      thread: buildThread([
        buildUserTurn("turn-1", "First prompt"),
        buildTurn("turn-2"),
        buildUserTurn("turn-3", "Second prompt"),
      ]),
    });

    const jumpButtons = screen.getAllByRole("button", {
      name: "Jump to previous user turn",
    });
    expect(jumpButtons).toHaveLength(1);

    fireEvent.click(jumpButtons[0]!);

    expect(scrollToItemMock).toHaveBeenCalledWith(0);
    await waitFor(() => {
      expect(scrollIntoViewMock).toHaveBeenCalledWith({
        behavior: "smooth",
        block: "start",
      });
      expect(scrollIntoViewMock.mock.instances[0]?.id).toBe(
        "user-message-anchor-turn-1-user",
      );
    });
  });
});
