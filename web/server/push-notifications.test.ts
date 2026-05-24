import { describe, expect, it } from "vitest";

import type { JsonRpcRequestMessage } from "../src/lib/jsonrpc";
import type { ServerNotification, ThreadItem } from "../src/types/protocol";

import {
  pushMessageForServerNotification,
  pushMessageForServerRequest,
} from "./push-notifications";

function turnCompletedNotification(items: ThreadItem[]): ServerNotification {
  return {
    method: "turn/completed",
    params: {
      threadId: "thread-1",
      turn: {
        completedAt: 2,
        durationMs: 1,
        error: null,
        id: "turn-1",
        items,
        itemsView: "full",
        startedAt: 1,
        status: "completed",
      },
    },
  };
}

function errorNotification(willRetry: boolean): ServerNotification {
  return {
    method: "error",
    params: {
      error: {
        additionalDetails: null,
        codexErrorInfo: null,
        message: "Reconnecting... 1/5",
      },
      threadId: "thread-1",
      turnId: "turn-1",
      willRetry,
    },
  };
}

describe("push notification payloads", () => {
  it("uses the final agent message as the completed-turn body", () => {
    expect(
      pushMessageForServerNotification(
        turnCompletedNotification([
          {
            id: "agent-1",
            memoryCitation: null,
            phase: "commentary",
            text: "I am still working on it.",
            type: "agentMessage",
          },
          {
            id: "agent-2",
            memoryCitation: null,
            phase: "final_answer",
            text: "Finished the notification payload change.\n\nTests pass.",
            type: "agentMessage",
          },
        ]),
      ),
    ).toMatchObject({
      body: "Finished the notification payload change. Tests pass.",
      title: "Codex finished",
      url: "/threads/thread-1",
    });
  });

  it("uses relay-provided agent text when turn-completed items are empty", () => {
    expect(
      pushMessageForServerNotification(turnCompletedNotification([]), {
        completedTurnAgentMessage:
          "The agent finished from relay-cached message text.",
      }),
    ).toMatchObject({
      body: "The agent finished from relay-cached message text.",
      title: "Codex finished",
    });
  });

  it("skips retry-progress errors", () => {
    expect(
      pushMessageForServerNotification(errorNotification(true)),
    ).toBeNull();
  });

  it("includes non-retrying errors", () => {
    expect(
      pushMessageForServerNotification(errorNotification(false)),
    ).toMatchObject({
      body: "Reconnecting... 1/5",
      tag: "codex-turn-thread-1-turn-1-failed",
      title: "Codex hit an error",
      url: "/threads/thread-1",
    });
  });

  it("includes command approval reason and command", () => {
    const request: JsonRpcRequestMessage = {
      id: "request-1",
      method: "item/commandExecution/requestApproval",
      params: {
        command: "curl https://example.com",
        itemId: "item-1",
        reason: "Needs network access",
        threadId: "thread-1",
        turnId: "turn-1",
      },
    };

    expect(pushMessageForServerRequest(request)).toMatchObject({
      body: "Needs network access. Command: curl https://example.com",
      title: "Codex needs command approval",
      url: "/threads/thread-1",
    });
  });

  it("includes file approval reason and requested root", () => {
    const request: JsonRpcRequestMessage = {
      id: "request-1",
      method: "item/fileChange/requestApproval",
      params: {
        grantRoot: "/home/warren/project",
        itemId: "item-1",
        reason: "Needs to edit project files",
        threadId: "thread-1",
        turnId: "turn-1",
      },
    };

    expect(pushMessageForServerRequest(request)).toMatchObject({
      body: "Needs to edit project files. Write root: /home/warren/project",
      title: "Codex needs file approval",
      url: "/threads/thread-1",
    });
  });

  it("includes permission approval reason and requested access", () => {
    const request: JsonRpcRequestMessage = {
      id: "request-1",
      method: "item/permissions/requestApproval",
      params: {
        itemId: "item-1",
        permissions: {
          fileSystem: {
            read: ["/tmp/input"],
            write: ["/tmp/output"],
          },
          network: {
            enabled: true,
          },
        },
        reason: "Needs broader access for the next command",
        threadId: "thread-1",
        turnId: "turn-1",
      },
    };

    expect(pushMessageForServerRequest(request)).toMatchObject({
      body: "Needs broader access for the next command. Network access. Read: /tmp/input. Write: /tmp/output",
      title: "Codex needs permission",
      url: "/threads/thread-1",
    });
  });
});
