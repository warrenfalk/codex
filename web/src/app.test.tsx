import type { ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BrowserRouter, MemoryRouter } from "react-router";

import type { Thread, Turn } from "@/types/protocol";

import { App } from "./app";

const backendStore = vi.hoisted(() => ({
  getThreadListSnapshot: vi.fn(),
  getThreadSnapshot: vi.fn(),
  interruptTurn: vi.fn(),
  renameThread: vi.fn(),
  refreshThreadList: vi.fn(),
  respondToServerRequest: vi.fn(),
  setForegroundThreadId: vi.fn(),
  sendPrompt: vi.fn(),
  subscribeThread: vi.fn(),
  subscribeThreadList: vi.fn(),
}));

vi.mock("@/lib/backend-store", () => backendStore);

vi.mock("react-bottom-anchored-list", async () => {
  const React = await import("react");

  return {
    BottomAnchoredList: React.forwardRef(function MockBottomAnchoredList(
      {
        items,
        renderItem,
      }: {
        items: Turn[];
        renderItem: (item: Turn) => ReactNode;
      },
      ref: React.ForwardedRef<{
        scrollToEnd: () => void;
        scrollToItem: (index: number) => void;
      }>,
    ) {
      React.useImperativeHandle(ref, () => ({
        scrollToEnd: () => undefined,
        scrollToItem: () => undefined,
      }));

      return <div>{items.map(renderItem)}</div>;
    }),
  };
});

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

function makeThread(id: string): Thread {
  return {
    id,
    forkedFromId: null,
    preview: "Continue the routing work",
    ephemeral: false,
    modelProvider: "openai",
    createdAt: 1,
    updatedAt: 2,
    status: { type: "idle" },
    path: null,
    cwd: "/workspace",
    cliVersion: "0.0.0",
    source: "appServer",
    agentNickname: null,
    agentRole: null,
    gitInfo: null,
    name: "Routing thread",
    turns: [],
  };
}

function renderApp(initialEntries: string[]) {
  render(
    <MemoryRouter initialEntries={initialEntries}>
      <App />
    </MemoryRouter>,
  );
}

function renderBrowserApp(pathname: string) {
  window.history.replaceState(null, "", pathname);
  render(
    <BrowserRouter>
      <App />
    </BrowserRouter>,
  );
}

describe("App routing", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    const thread = makeThread("thread-1");

    backendStore.getThreadListSnapshot.mockReturnValue({
      connectionError: null,
      connectionState: "connected",
      initializeSummary: "codex 0.0.0 on linux",
      loading: false,
      previewsByThreadId: {},
      threads: [thread],
      warnings: [],
    });
    backendStore.getThreadSnapshot.mockReturnValue({
      connectionError: null,
      connectionState: "connected",
      initializeSummary: "codex 0.0.0 on linux",
      itemRuntimeText: {},
      loading: false,
      pendingRequests: [],
      thread,
      threadId: thread.id,
      warnings: [],
    });
    backendStore.interruptTurn.mockResolvedValue(undefined);
    backendStore.renameThread.mockResolvedValue(undefined);
    backendStore.refreshThreadList.mockResolvedValue(undefined);
    backendStore.respondToServerRequest.mockResolvedValue(undefined);
    backendStore.setForegroundThreadId.mockResolvedValue(undefined);
    backendStore.sendPrompt.mockResolvedValue(undefined);
    backendStore.subscribeThread.mockReturnValue(() => undefined);
    backendStore.subscribeThreadList.mockReturnValue(() => undefined);
  });

  it("pushes a thread route when selecting a thread and pops it on in-app back", () => {
    renderApp(["/"]);

    fireEvent.click(screen.getByRole("button", { name: /routing thread/i }));

    expect(
      screen.getByRole("heading", { name: "Routing thread" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Back to threads" }));

    expect(
      screen.getByRole("heading", { name: "Threads" }),
    ).toBeInTheDocument();
  });

  it("returns to the thread list when browser back pops the thread route", async () => {
    renderBrowserApp("/");

    fireEvent.click(screen.getByRole("button", { name: /routing thread/i }));

    expect(window.location.pathname).toBe("/threads/thread-1");

    act(() => {
      window.history.back();
    });

    await waitFor(() => {
      expect(window.location.pathname).toBe("/");
    });
    expect(
      screen.getByRole("heading", { name: "Threads" }),
    ).toBeInTheDocument();
  });

  it("replaces direct thread links with the thread list on in-app back", () => {
    renderApp(["/threads/thread-1"]);

    fireEvent.click(screen.getByRole("button", { name: "Back to threads" }));

    expect(
      screen.getByRole("heading", { name: "Threads" }),
    ).toBeInTheDocument();
  });
});
