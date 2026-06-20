import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ThreadItem } from "@/types/protocol";

import { ThreadItemView } from "./thread-item-view";

describe("ThreadItemView", () => {
  it("renders a previous-user-turn jump button when requested", () => {
    const item: ThreadItem = {
      type: "userMessage",
      id: "user-jump-1",
      clientId: null,
      content: [
        {
          type: "text",
          text: "Hello",
          text_elements: [],
        },
      ],
    };
    const onJumpToPreviousUserTurn = vi.fn();

    render(
      <ThreadItemView
        item={item}
        onJumpToPreviousUserTurn={onJumpToPreviousUserTurn}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Jump to previous user turn" }),
    );

    expect(onJumpToPreviousUserTurn).toHaveBeenCalledTimes(1);
  });

  it("renders user text input as markdown", () => {
    const item: ThreadItem = {
      type: "userMessage",
      id: "user-1",
      clientId: null,
      content: [
        {
          type: "text",
          text: "Hello **user**\n\n- first\n- second",
          text_elements: [],
        },
      ],
    };

    render(<ThreadItemView item={item} />);

    expect(screen.getByText("user")).toHaveAttribute(
      "data-streamdown",
      "strong",
    );
    expect(
      screen.getAllByRole("listitem").map((entry) => entry.textContent),
    ).toEqual(["first", "second"]);
  });

  it("renders Codex output as markdown", () => {
    const item: ThreadItem = {
      type: "agentMessage",
      id: "agent-1",
      text: "Use `codex` with [docs](https://example.test).",
      phase: null,
      memoryCitation: null,
    };

    render(<ThreadItemView item={item} />);

    const link = screen.getByRole("link", { name: "docs" });
    expect(link).toHaveAttribute("href", "https://example.test/");
    expect(
      within(screen.getByText("codex").closest(".message-markdown")!).getByText(
        "codex",
      ),
    ).toHaveAttribute("data-streamdown", "inline-code");
  });

  it("pushes local source links through the source file callback", () => {
    const item: ThreadItem = {
      type: "agentMessage",
      id: "agent-source-link-1",
      text: "Open [the component](src/components/thread-item-view.tsx:42).",
      phase: null,
      memoryCitation: null,
    };
    const onNavigate = vi.fn();

    render(
      <ThreadItemView
        item={item}
        sourceFileLinks={{
          onNavigate,
          root: "/workspace",
          threadId: "thread-1",
        }}
      />,
    );

    const link = screen.getByRole("link", { name: "the component" });
    expect(link).toHaveAttribute(
      "href",
      "/files?path=src%2Fcomponents%2Fthread-item-view.tsx&threadId=thread-1&line=42",
    );

    fireEvent.click(link);

    expect(onNavigate).toHaveBeenCalledWith(
      "/files?path=src%2Fcomponents%2Fthread-item-view.tsx&threadId=thread-1&line=42",
    );
  });

  it("renders fenced code blocks with separate line elements", () => {
    const item: ThreadItem = {
      type: "agentMessage",
      id: "agent-code-1",
      text: [
        "```sql",
        "update animals",
        "set lot_id = 16768",
        "where org_id = 219",
        "```",
      ].join("\n"),
      phase: null,
      memoryCitation: null,
    };

    const { container } = render(<ThreadItemView item={item} />);

    const lines = container.querySelectorAll(
      '[data-streamdown="code-block-body"] code > span',
    );
    expect(Array.from(lines).map((line) => line.textContent)).toEqual([
      "update animals",
      "set lot_id = 16768",
      "where org_id = 219",
    ]);
  });

  it("renders command executions collapsed by default", () => {
    const item: ThreadItem = {
      type: "commandExecution",
      id: "command-1",
      command: "pnpm run typecheck",
      cwd: "/workspace",
      processId: null,
      source: "agent",
      status: "completed",
      commandActions: [],
      aggregatedOutput: "typecheck passed",
      exitCode: 0,
      durationMs: 10,
    };

    render(<ThreadItemView item={item} />);

    expect(screen.getByText("EXEC")).toBeInTheDocument();
    expect(screen.getAllByText("pnpm run typecheck")).toHaveLength(2);

    const details = screen
      .getByText("EXEC")
      .closest("details") as HTMLDetailsElement;
    expect(details.open).toBe(false);
    expect(details.querySelector(".collapsible-summary-icon")).not.toBeNull();

    fireEvent.click(screen.getByText("EXEC"));

    expect(details.open).toBe(true);
  });

  it("renders file changes collapsed by default", () => {
    const item: ThreadItem = {
      type: "fileChange",
      id: "file-change-1",
      status: "completed",
      changes: [
        {
          path: "src/components/thread-item-view.tsx",
          kind: { type: "update", move_path: null },
          diff: "@@ -1 +1 @@",
        },
      ],
    };

    render(<ThreadItemView item={item} />);

    expect(screen.getByText("EDIT")).toBeInTheDocument();
    expect(screen.getByText("src/components/")).toHaveClass(
      "file-path-directory",
    );
    expect(screen.getByText("thread-item-view.tsx")).toHaveClass(
      "file-path-basename",
    );

    const details = screen
      .getByText("EDIT")
      .closest("details") as HTMLDetailsElement;
    expect(details.open).toBe(false);
    expect(details.querySelector(".collapsible-summary-icon")).not.toBeNull();

    fireEvent.click(screen.getByText("EDIT"));

    expect(details.open).toBe(true);
  });
});
