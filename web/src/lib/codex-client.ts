import type {
  AnyServerRequest,
  ClientInfo,
  InitializeCapabilities,
  InitializeResponse,
  RequestId,
  ServerNotification,
  Thread,
  ThreadListResponse,
  ThreadResumeResponse,
  ThreadSetNameResponse,
  ThreadTurnsListResponse,
  Turn,
  TurnInterruptResponse,
  TurnStartResponse,
} from "@/types/protocol";

import { isKnownServerRequestMethod } from "@/types/protocol";

import { type IncomingJsonRpcMessage, JsonRpcConnection } from "./jsonrpc";
import {
  isProxyThreadListUpdatedNotification,
  type ProxyThreadListResponse,
  type ProxyThreadListSnapshot,
  type ProxyThreadListUpdatedNotification,
} from "./proxy-protocol";

export type BackendMessage =
  | AnyServerRequest
  | ProxyThreadListUpdatedNotification
  | ServerNotification;

type MessageListener = (message: BackendMessage) => void;
type StatusListener = (status: string, error?: string) => void;
type ThreadTurnsSortDirection = "asc" | "desc";

export type ThreadTurnsPageRequest = {
  cursor: string | null;
  sortDirection: ThreadTurnsSortDirection;
};

export type ThreadTurnsPage = {
  nextCursor: string | null;
  turns: Turn[];
};

const CLIENT_INFO: ClientInfo = {
  name: "codex_web",
  title: "Codex Web",
  version: "0.0.0",
};

const CAPABILITIES: InitializeCapabilities = {
  experimentalApi: true,
};

function isServerRequest(
  message: IncomingJsonRpcMessage,
): message is AnyServerRequest {
  return "id" in message;
}

function isMethodNotFoundError(
  error: unknown,
): error is { code: -32601; message?: string } {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === -32601
  );
}

export class CodexClient {
  private readonly rpc = new JsonRpcConnection();
  private readonly listeners = new Set<MessageListener>();

  constructor(private readonly endpoint: string) {
    this.rpc.subscribe((message) => {
      const payload = isServerRequest(message)
        ? isKnownServerRequestMethod(message.method)
          ? message
          : { ...message, unknown: true as const }
        : isProxyThreadListUpdatedNotification(message)
          ? message
          : (message as ServerNotification);

      for (const listener of this.listeners) {
        listener(payload);
      }
    });
  }

  async connect(): Promise<InitializeResponse> {
    await this.rpc.connect(this.endpoint);

    const response = await this.rpc.request<InitializeResponse>("initialize", {
      clientInfo: CLIENT_INFO,
      capabilities: CAPABILITIES,
    });
    this.rpc.notify("initialized", {});
    return response;
  }

  disconnect(): void {
    this.rpc.disconnect();
  }

  subscribe(listener: MessageListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  subscribeStatus(listener: StatusListener): () => void {
    return this.rpc.subscribeStatus(listener);
  }

  async listAllThreads(): Promise<ProxyThreadListSnapshot> {
    const threads: Thread[] = [];
    const previewsByThreadId: Record<string, string> = {};
    let cursor: string | null = null;

    do {
      const response: ProxyThreadListResponse | ThreadListResponse =
        await this.rpc.request<ProxyThreadListResponse | ThreadListResponse>(
          "thread/list",
          {
            cursor,
            limit: 50,
            sortDirection: "desc",
          },
        );
      threads.push(...response.data);
      if ("previewsByThreadId" in response) {
        Object.assign(previewsByThreadId, response.previewsByThreadId);
      }
      cursor = response.nextCursor;
    } while (cursor);

    return {
      previewsByThreadId,
      threads,
    };
  }

  async listThreadTurnsPage(
    threadId: string,
    { cursor, sortDirection }: ThreadTurnsPageRequest,
  ): Promise<ThreadTurnsPage> {
    try {
      const response: ThreadTurnsListResponse =
        await this.rpc.request<ThreadTurnsListResponse>("thread/turns/list", {
          cursor,
          limit: 100,
          sortDirection,
          threadId,
        });
      return {
        nextCursor: response.nextCursor,
        turns:
          sortDirection === "desc"
            ? response.data.slice().reverse()
            : response.data,
      };
    } catch (error) {
      if (isMethodNotFoundError(error)) {
        return {
          nextCursor: null,
          turns: [],
        };
      }
      throw error;
    }
  }

  async resumeThread(threadId: string): Promise<Thread> {
    const response = await this.rpc.request<ThreadResumeResponse>(
      "thread/resume",
      {
        excludeTurns: true,
        threadId,
      },
    );
    return response.thread;
  }

  async sendPrompt(threadId: string, text: string): Promise<void> {
    await this.rpc.request<TurnStartResponse>("turn/start", {
      threadId,
      input: [
        {
          type: "text",
          text,
          text_elements: [],
        },
      ],
    });
  }

  async renameThread(threadId: string, name: string): Promise<void> {
    await this.rpc.request<ThreadSetNameResponse>("thread/name/set", {
      threadId,
      name,
    });
  }

  async interruptTurn(threadId: string, turnId: string): Promise<void> {
    await this.rpc.request<TurnInterruptResponse>("turn/interrupt", {
      threadId,
      turnId,
    });
  }

  respondToServerRequest(id: RequestId, response: unknown): void {
    this.rpc.respond(id, response);
  }
}
