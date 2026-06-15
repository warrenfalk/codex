import type {
  AnyServerRequest,
  ClientInfo,
  ExperimentalThreadResumeParams,
  ExperimentalThreadResumeResponse,
  ExperimentalThreadTurnsListParams,
  InitializeCapabilities,
  InitializeResponse,
  RequestId,
  ServerNotification,
  Thread,
  ThreadArchiveResponse,
  ThreadListResponse,
  ThreadSetNameResponse,
  ThreadTurnsListResponse,
  Turn,
  TurnInterruptResponse,
  TurnStartResponse,
} from "@/types/protocol";

import { isKnownServerRequestMethod } from "@/types/protocol";

import { type IncomingJsonRpcMessage, JsonRpcConnection } from "./jsonrpc";
import {
  PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD,
  isProxyThreadListUpdatedNotification,
  type ProxyThreadListResponse,
  type ProxyThreadListSnapshot,
  type ProxyThreadListUpdatedNotification,
} from "./proxy-protocol";
import { getPushSubscriptionEndpoint } from "./push-notifications";

export type BackendMessage =
  | AnyServerRequest
  | ProxyThreadListUpdatedNotification
  | ServerNotification;

type MessageListener = (message: BackendMessage) => void;
type StatusListener = (status: string, error?: string) => void;
type ThreadTurnsSortDirection = "asc" | "desc";

export type ThreadTurnsPageRequest = {
  cursor: string | null;
  limit?: number | null;
  sortDirection: ThreadTurnsSortDirection;
};

export type ThreadTurnsPage = {
  nextCursor: string | null;
  turns: Turn[];
};

export type ThreadResumeResult = {
  initialTurnsPage: ThreadTurnsPage | null;
  thread: Thread;
};

const CLIENT_INFO: ClientInfo = {
  name: "codex_web",
  title: "Codex Web",
  version: "0.0.0",
};

const CAPABILITIES: InitializeCapabilities = {
  experimentalApi: true,
  requestAttestation: false,
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

function normalizeThreadTurnsPage(
  response: ThreadTurnsListResponse,
  sortDirection: ThreadTurnsSortDirection,
): ThreadTurnsPage {
  return {
    nextCursor: response.nextCursor,
    turns:
      sortDirection === "desc"
        ? response.data.slice().reverse()
        : response.data,
  };
}

export class CodexClient {
  private foregroundThreadId: string | null = null;
  private pushSubscriptionEndpoint: string | null = null;
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
    this.pushSubscriptionEndpoint = await this.readPushSubscriptionEndpoint();
    this.rpc.notify("initialized", this.clientStateParams());
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
    const threadActivityByThreadId: ProxyThreadListSnapshot["threadActivityByThreadId"] =
      {};
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
      if ("threadActivityByThreadId" in response) {
        Object.assign(
          threadActivityByThreadId,
          response.threadActivityByThreadId,
        );
      }
      cursor = response.nextCursor;
    } while (cursor);

    return {
      previewsByThreadId,
      threadActivityByThreadId,
      threads,
    };
  }

  async listThreadTurnsPage(
    threadId: string,
    { cursor, limit = 100, sortDirection }: ThreadTurnsPageRequest,
  ): Promise<ThreadTurnsPage> {
    try {
      const params: ExperimentalThreadTurnsListParams = {
        cursor,
        itemsView: "full",
        limit,
        sortDirection,
        threadId,
      };
      const response: ThreadTurnsListResponse =
        await this.rpc.request<ThreadTurnsListResponse>(
          "thread/turns/list",
          params,
        );
      return normalizeThreadTurnsPage(response, sortDirection);
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

  async resumeThread(threadId: string): Promise<ThreadResumeResult> {
    const params: ExperimentalThreadResumeParams = {
      excludeTurns: true,
      initialTurnsPage: {
        itemsView: "full",
        limit: 1,
        sortDirection: "desc",
      },
      threadId,
    };
    const response = await this.rpc.request<ExperimentalThreadResumeResponse>(
      "thread/resume",
      params,
    );
    return {
      initialTurnsPage: response.initialTurnsPage
        ? normalizeThreadTurnsPage(response.initialTurnsPage, "desc")
        : null,
      thread: response.thread,
    };
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

  async archiveThread(threadId: string): Promise<void> {
    await this.rpc.request<ThreadArchiveResponse>("thread/archive", {
      threadId,
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

  setPushSubscriptionEndpoint(endpoint: string | null): void {
    this.pushSubscriptionEndpoint = endpoint;
    this.notifyClientState();
  }

  setForegroundThreadId(threadId: string | null): void {
    this.foregroundThreadId = threadId;
    this.notifyClientState();
  }

  private async readPushSubscriptionEndpoint(): Promise<string | null> {
    try {
      return await getPushSubscriptionEndpoint();
    } catch (error) {
      console.warn("Could not read active push subscription endpoint.", error);
      return null;
    }
  }

  private clientStateParams(): {
    foregroundThreadId: string | null;
    pushSubscriptionEndpoint: string | null;
  } {
    return {
      foregroundThreadId: this.foregroundThreadId,
      pushSubscriptionEndpoint: this.pushSubscriptionEndpoint,
    };
  }

  private notifyClientState(): void {
    if (!this.rpc.isOpen()) {
      return;
    }

    this.rpc.notify(
      PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD,
      this.clientStateParams(),
    );
  }
}
