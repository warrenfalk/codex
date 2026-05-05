import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Thread } from "@/types/protocol";

import { ThreadList } from "./thread-list";

afterEach(() => {
  cleanup();
});

function buildThread(overrides: Partial<Thread> = {}): Thread {
  return {
    id: "thread-1",
    forkedFromId: null,
    preview: "Original prompt",
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
    name: null,
    turns: [],
    ...overrides,
  };
}

describe("ThreadList", () => {
  it("renders the latest sender preview when available", () => {
    const onRefresh = vi.fn();
    const onSelect = vi.fn();

    render(
      <ThreadList
        loading={false}
        onRefresh={onRefresh}
        onSelect={onSelect}
        previewsByThreadId={{ "thread-1": "**Codex:** Latest `response`" }}
        threads={[buildThread()]}
      />,
    );

    expect(screen.getByText("Codex:")).toHaveAttribute(
      "data-streamdown",
      "strong",
    );
    expect(screen.getByText("response")).toHaveAttribute(
      "data-streamdown",
      "inline-code",
    );
    expect(screen.getByText("Original prompt")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /original prompt/i }));

    expect(onSelect).toHaveBeenCalledWith("thread-1");
  });

  it("filters threads by cwd and branch metadata", () => {
    render(
      <ThreadList
        loading={false}
        onRefresh={vi.fn()}
        onSelect={vi.fn()}
        previewsByThreadId={{}}
        threads={[
          buildThread({
            id: "thread-1",
            cwd: "/workspace/codex-remote-control-web",
            gitInfo: {
              branch: "feature/search-threads",
              originUrl: "git@example.com:warren/codex.git",
              sha: "abc123",
            },
            name: "Remote control thread",
          }),
          buildThread({
            id: "thread-2",
            cwd: "/tmp/other-project",
            name: "Unrelated thread",
          }),
        ]}
      />,
    );

    fireEvent.change(
      screen.getByRole("searchbox", { name: "Search threads" }),
      {
        target: { value: "branch:feature/search-threads" },
      },
    );

    expect(screen.getByText("Remote control thread")).toBeInTheDocument();
    expect(screen.queryByText("Unrelated thread")).not.toBeInTheDocument();

    fireEvent.change(
      screen.getByRole("searchbox", { name: "Search threads" }),
      {
        target: { value: "cwd:/tmp/other-project" },
      },
    );

    expect(screen.queryByText("Remote control thread")).not.toBeInTheDocument();
    expect(screen.getByText("Unrelated thread")).toBeInTheDocument();
  });

  it("keeps list actions behind the thread actions menu", () => {
    const onRefresh = vi.fn();

    render(
      <ThreadList
        loading={false}
        onRefresh={onRefresh}
        onSelect={vi.fn()}
        previewsByThreadId={{}}
        threads={[buildThread()]}
      />,
    );

    const actionsTrigger = screen.getByLabelText("Thread list actions");

    expect(actionsTrigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();

    fireEvent.click(actionsTrigger);

    expect(actionsTrigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(onRefresh).toHaveBeenCalledOnce();
    expect(actionsTrigger).toHaveAttribute("aria-expanded", "false");
  });
});
