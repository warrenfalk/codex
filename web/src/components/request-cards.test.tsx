import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ServerRequest } from "../../../codex-rs/app-server-protocol/schema/typescript/ServerRequest";

import { RequestCard } from "./request-cards";

describe("RequestCard", () => {
  afterEach(() => {
    cleanup();
  });

  it("uses default command approval decisions when the server omits them", async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn();
    const request: ServerRequest = {
      method: "item/commandExecution/requestApproval",
      id: 2,
      params: {
        threadId: "thr_1",
        turnId: "turn_1",
        itemId: "item_1",
        command: "pnpm build",
      },
    };

    render(<RequestCard onRespond={onRespond} request={request} />);

    const acceptButton = screen.getByRole("button", { name: "Yes, proceed" });
    expect(acceptButton).toHaveClass("approval-decision-yes");
    await user.click(acceptButton);

    expect(onRespond).toHaveBeenCalledWith(expect.objectContaining({ id: 2 }), {
      decision: "accept",
    });
  });

  it("labels persistent command approvals with the command prefix", async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn();
    const persistentDecision = {
      acceptWithExecpolicyAmendment: {
        execpolicy_amendment: ["printf", "approval protocol smoke test\n"],
      },
    };
    const request: ServerRequest = {
      method: "item/commandExecution/requestApproval",
      id: 5,
      params: {
        threadId: "thr_1",
        turnId: "turn_1",
        itemId: "item_1",
        command: 'printf "approval protocol smoke test\\n"',
        availableDecisions: ["accept", persistentDecision, "cancel"],
      },
    };

    render(<RequestCard onRespond={onRespond} request={request} />);

    const label =
      'Yes, and don\'t ask again for commands that start with `printf "approval protocol smoke test\\n"`';
    await user.click(
      screen.getByRole("button", {
        name: label,
      }),
    );

    expect(onRespond).toHaveBeenCalledWith(expect.objectContaining({ id: 5 }), {
      decision: persistentDecision,
    });
    expect(
      screen.getByRole("button", {
        name: "No, and tell Codex what to do differently",
      }),
    ).toHaveClass("approval-decision-no");
  });

  it("submits permission grants with selected scope", async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn();
    const request: ServerRequest = {
      method: "item/permissions/requestApproval",
      id: 3,
      params: {
        threadId: "thr_1",
        turnId: "turn_1",
        itemId: "item_1",
        reason: "Need extra write access",
        permissions: {
          network: { enabled: true },
          fileSystem: {
            read: null,
            write: ["/tmp/project"],
          },
        },
      },
    };

    render(<RequestCard onRespond={onRespond} request={request} />);

    await user.selectOptions(screen.getByRole("combobox"), "session");
    await user.click(screen.getByRole("button", { name: "Submit grant" }));

    expect(onRespond).toHaveBeenCalledWith(
      expect.objectContaining({ id: 3 }),
      expect.objectContaining({
        scope: "session",
      }),
    );
  });

  it("submits canonical filesystem permission entries", async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn();
    const entry = {
      path: {
        type: "glob_pattern",
        pattern: "/tmp/project/**/*.rs",
      },
      access: "write",
    };
    const request = {
      method: "item/permissions/requestApproval",
      id: 4,
      params: {
        threadId: "thr_1",
        turnId: "turn_1",
        itemId: "item_1",
        reason: "Need canonical file access",
        permissions: {
          network: null,
          fileSystem: {
            read: null,
            write: null,
            globScanMaxDepth: 3,
            entries: [entry],
          },
        },
      },
    } as unknown as ServerRequest;

    render(<RequestCard onRespond={onRespond} request={request} />);

    expect(
      screen.getByText("write: glob /tmp/project/**/*.rs"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Submit grant" }));

    expect(onRespond).toHaveBeenCalledWith(
      expect.objectContaining({ id: 4 }),
      expect.objectContaining({
        permissions: {
          fileSystem: {
            read: null,
            write: null,
            globScanMaxDepth: 3,
            entries: [entry],
          },
        },
      }),
    );
  });

  it("submits tool answers", async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn();
    const request: ServerRequest = {
      method: "item/tool/requestUserInput",
      id: 9,
      params: {
        threadId: "thr_1",
        turnId: "turn_1",
        itemId: "item_2",
        questions: [
          {
            id: "q1",
            header: "Choice",
            question: "Pick one",
            options: [
              {
                label: "Recommended",
                description: "Best option",
              },
            ],
            isOther: false,
            isSecret: false,
          },
        ],
      },
    };

    render(<RequestCard onRespond={onRespond} request={request} />);

    await user.click(screen.getByRole("button", { name: "Recommended" }));
    await user.click(screen.getByRole("button", { name: "Submit answers" }));

    expect(onRespond).toHaveBeenCalledWith(expect.objectContaining({ id: 9 }), {
      answers: {
        q1: {
          answers: ["Recommended"],
        },
      },
    });
  });
});
