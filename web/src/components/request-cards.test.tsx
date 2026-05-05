import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ServerRequest } from "../../../codex-rs/app-server-protocol/schema/typescript/ServerRequest";

import { RequestCard } from "./request-cards";

describe("RequestCard", () => {
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
