import type { IncomingMessage, Server as HttpServer } from "node:http";

import { WebSocket, WebSocketServer } from "ws";

import type {
  InitializeResponse,
  RequestId,
  ServerNotification,
  Thread,
  ThreadListResponse,
} from "../src/types/protocol";
import {
  PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD,
  PROXY_THREAD_LIST_UPDATED_METHOD,
  type ProxyThreadListResponse,
  type ProxyThreadListUpdatedNotification,
} from "../src/lib/proxy-protocol";

import type { PushNotifier, PushNotifyOptions } from "./push-service.js";
import { ThreadCache } from "./thread-cache";

type JsonRpcRequestMessage = {
  id: RequestId;
  method: string;
  params: unknown;
};

type JsonRpcNotificationMessage = {
  method: string;
  params?: unknown;
};

type JsonRpcResponseMessage = {
  id: RequestId;
  result: unknown;
};

type JsonRpcErrorShape = {
  code: number;
  message: string;
  data?: unknown;
};

type JsonRpcErrorMessage = {
  id: RequestId;
  error: JsonRpcErrorShape;
};

type JsonRpcMessage =
  | JsonRpcRequestMessage
  | JsonRpcNotificationMessage
  | JsonRpcResponseMessage
  | JsonRpcErrorMessage;

type PendingProxyRequest = {
  reject: (reason?: unknown) => void;
  resolve: (value: unknown) => void;
};

type ForwardedBrowserRequest = {
  browserRequestId: RequestId;
  method: string;
  socket: WebSocket;
};

const CLIENT_INFO = {
  name: "codex_web_proxy",
  title: "Codex Web Proxy",
  version: "0.0.0",
};

const CAPABILITIES = {
  experimentalApi: true,
};

function closeIfOpen(
  socket: WebSocket,
  code?: number,
  reason?: string | Buffer,
): void {
  if (socket.readyState === WebSocket.OPEN) {
    const normalizedReason =
      typeof reason === "string" ? reason : reason?.toString();
    const hasSendableCode =
      code === 1000 ||
      (code !== undefined &&
        ((code >= 1001 &&
          code <= 1014 &&
          code !== 1004 &&
          code !== 1005 &&
          code !== 1006) ||
          (code >= 3000 && code <= 4999)));

    if (hasSendableCode) {
      socket.close(code, normalizedReason);
    } else {
      socket.close();
    }
    return;
  }

  if (socket.readyState === WebSocket.CONNECTING) {
    socket.terminate();
  }
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isRequestMessage(value: unknown): value is JsonRpcRequestMessage {
  return isObject(value) && "id" in value && "method" in value;
}

function isNotificationMessage(
  value: unknown,
): value is JsonRpcNotificationMessage {
  return isObject(value) && "method" in value && !("id" in value);
}

function isResponseMessage(value: unknown): value is JsonRpcResponseMessage {
  return isObject(value) && "id" in value && "result" in value;
}

function isErrorMessage(value: unknown): value is JsonRpcErrorMessage {
  return isObject(value) && "id" in value && "error" in value;
}

function requestIdKey(id: RequestId): string {
  return typeof id === "string" ? `string:${id}` : `number:${id}`;
}

function pushSubscriptionEndpointFromParams(params: unknown): string | null {
  if (!isObject(params)) {
    return null;
  }

  const endpoint = params.pushSubscriptionEndpoint;
  return typeof endpoint === "string" && endpoint.length > 0 ? endpoint : null;
}

function canSend(socket: WebSocket): boolean {
  return socket.readyState === WebSocket.OPEN;
}

function sendJson(socket: WebSocket, message: JsonRpcMessage): void {
  if (!canSend(socket)) {
    return;
  }
  socket.send(JSON.stringify(message));
}

function parseJsonRpcMessage(
  rawData: WebSocket.RawData,
): JsonRpcMessage | null {
  const rawText =
    typeof rawData === "string"
      ? rawData
      : Array.isArray(rawData)
        ? Buffer.concat(rawData).toString("utf8")
        : rawData.toString();

  let parsed: unknown;
  try {
    parsed = JSON.parse(rawText);
  } catch {
    return null;
  }

  if (
    isRequestMessage(parsed) ||
    isNotificationMessage(parsed) ||
    isResponseMessage(parsed) ||
    isErrorMessage(parsed)
  ) {
    return parsed;
  }

  return null;
}

function isInitializeResponse(value: unknown): value is InitializeResponse {
  return (
    isObject(value) &&
    typeof value.userAgent === "string" &&
    typeof value.codexHome === "string" &&
    typeof value.platformFamily === "string" &&
    typeof value.platformOs === "string"
  );
}

function isThread(value: unknown): value is Thread {
  return isObject(value) && typeof value.id === "string";
}

function extractThreadFromResponse(result: unknown): Thread | null {
  if (!isObject(result) || !("thread" in result)) {
    return null;
  }

  const { thread } = result;
  return isThread(thread) ? thread : null;
}

async function openUpstreamSocket(backendUrl: string): Promise<WebSocket> {
  return await new Promise<WebSocket>((resolve, reject) => {
    const socket = new WebSocket(backendUrl);

    socket.once("open", () => resolve(socket));
    socket.once("error", (error) => reject(error));
  });
}

class RelayController {
  private connectPromise: Promise<void> | null = null;
  private initializeResponse: InitializeResponse | null = null;
  private nextUpstreamRequestId = 1;
  private readonly browserSockets = new Set<WebSocket>();
  private readonly cache = new ThreadCache();
  private readonly forwardedBrowserRequests = new Map<
    string,
    ForwardedBrowserRequest
  >();
  private readonly pendingProxyRequests = new Map<
    string,
    PendingProxyRequest
  >();
  private readonly pendingServerRequests = new Map<
    string,
    JsonRpcRequestMessage
  >();
  private readonly pendingServerRequestIds = new Set<string>();
  private readonly respondedServerRequestIds = new Set<string>();
  private readonly pushSubscriptionEndpointsBySocket = new Map<
    WebSocket,
    string
  >();
  private upstreamSocket: WebSocket | null = null;

  constructor(
    private readonly backendUrl: string,
    private readonly pushNotifier: PushNotifier | null,
  ) {}

  async start(): Promise<void> {
    await this.ensureUpstreamConnected();
  }

  attach(server: HttpServer): void {
    const relayServer = new WebSocketServer({ noServer: true });

    relayServer.on("connection", (socket) => {
      this.handleBrowserConnection(socket);
    });

    server.on("upgrade", (request, socket, head) => {
      const pathname = new URL(request.url ?? "/", "http://localhost").pathname;

      if (pathname !== "/rpc") {
        socket.destroy();
        return;
      }

      relayServer.handleUpgrade(
        request as IncomingMessage,
        socket,
        head,
        (webSocket) => {
          relayServer.emit("connection", webSocket, request);
        },
      );
    });
  }

  private async ensureUpstreamConnected(): Promise<void> {
    if (this.upstreamSocket && canSend(this.upstreamSocket)) {
      return;
    }

    if (this.connectPromise) {
      return this.connectPromise;
    }

    this.connectPromise = (async () => {
      const socket = await openUpstreamSocket(this.backendUrl);
      this.upstreamSocket = socket;

      socket.on("message", (rawData) => {
        this.handleUpstreamMessage(rawData);
      });
      socket.on("close", (code, reason) => {
        this.handleUpstreamDisconnect(code, reason.toString());
      });
      socket.on("error", (error) => {
        this.handleUpstreamDisconnect(1011, String(error));
      });

      const initializeResponse = await this.requestUpstream<InitializeResponse>(
        "initialize",
        {
          capabilities: CAPABILITIES,
          clientInfo: CLIENT_INFO,
        },
      );
      if (!isInitializeResponse(initializeResponse)) {
        throw new Error("proxy received an invalid initialize response");
      }

      this.initializeResponse = initializeResponse;
      this.sendUpstream({
        method: "initialized",
        params: {},
      });
      await this.reloadThreadCacheFromUpstream();
    })().finally(() => {
      this.connectPromise = null;
    });

    return this.connectPromise;
  }

  private async reloadThreadCacheFromUpstream(): Promise<void> {
    const threads: Thread[] = [];
    let cursor: string | null = null;

    do {
      const response: ThreadListResponse =
        await this.requestUpstream<ThreadListResponse>("thread/list", {
          cursor,
          limit: 100,
          sortDirection: "desc",
        });
      threads.push(...response.data);
      cursor = response.nextCursor;
    } while (cursor);

    this.cache.replaceThreads(threads);
  }

  private handleBrowserConnection(socket: WebSocket): void {
    this.browserSockets.add(socket);

    socket.on("message", (rawData) => {
      void this.handleBrowserMessage(socket, rawData);
    });

    socket.on("close", () => {
      this.browserSockets.delete(socket);
      this.pushSubscriptionEndpointsBySocket.delete(socket);
      for (const [requestKey, pending] of this.forwardedBrowserRequests) {
        if (pending.socket === socket) {
          this.forwardedBrowserRequests.delete(requestKey);
        }
      }
    });

    socket.on("error", () => {
      this.browserSockets.delete(socket);
      this.pushSubscriptionEndpointsBySocket.delete(socket);
      closeIfOpen(socket, 1011, "browser socket errored");
    });
  }

  private async handleBrowserMessage(
    socket: WebSocket,
    rawData: WebSocket.RawData,
  ): Promise<void> {
    const message = parseJsonRpcMessage(rawData);
    if (!message) {
      closeIfOpen(socket, 1003, "expected JSON-RPC text frames");
      return;
    }

    if (isResponseMessage(message) || isErrorMessage(message)) {
      await this.handleBrowserResponse(message);
      return;
    }

    if (isNotificationMessage(message)) {
      await this.handleBrowserNotification(socket, message);
      return;
    }

    await this.handleBrowserRequest(socket, message);
  }

  private async handleBrowserRequest(
    socket: WebSocket,
    message: JsonRpcRequestMessage,
  ): Promise<void> {
    await this.ensureUpstreamConnected();

    if (message.method === "initialize") {
      sendJson(socket, {
        id: message.id,
        result: this.initializeResponse,
      });
      return;
    }

    if (message.method === "thread/list") {
      sendJson(socket, {
        id: message.id,
        result: this.buildThreadListResponse(),
      });
      return;
    }

    const upstreamId = `browser:${this.nextUpstreamRequestId}`;
    this.nextUpstreamRequestId += 1;
    this.forwardedBrowserRequests.set(requestIdKey(upstreamId), {
      browserRequestId: message.id,
      method: message.method,
      socket,
    });
    this.sendUpstream({
      id: upstreamId,
      method: message.method,
      params: message.params,
    });
  }

  private async handleBrowserNotification(
    socket: WebSocket,
    message: JsonRpcNotificationMessage,
  ): Promise<void> {
    await this.ensureUpstreamConnected();

    if (message.method === "initialized") {
      this.updateSocketPushSubscriptionEndpoint(socket, message.params);
      this.sendThreadListSnapshotToSocket(socket);
      this.sendPendingServerRequestsToSocket(socket);
      return;
    }

    if (message.method === PROXY_PUSH_SUBSCRIPTION_UPDATED_METHOD) {
      this.updateSocketPushSubscriptionEndpoint(socket, message.params);
      return;
    }

    this.sendUpstream(message);
  }

  private async handleBrowserResponse(
    message: JsonRpcResponseMessage | JsonRpcErrorMessage,
  ): Promise<void> {
    await this.ensureUpstreamConnected();

    const requestKey = requestIdKey(message.id);
    if (
      !this.pendingServerRequestIds.has(requestKey) ||
      this.respondedServerRequestIds.has(requestKey)
    ) {
      return;
    }

    this.respondedServerRequestIds.add(requestKey);
    this.sendUpstream(message);
  }

  private handleUpstreamMessage(rawData: WebSocket.RawData): void {
    const message = parseJsonRpcMessage(rawData);
    if (!message) {
      this.handleUpstreamDisconnect(
        1011,
        "upstream sent an invalid JSON-RPC frame",
      );
      return;
    }

    if (isResponseMessage(message) || isErrorMessage(message)) {
      this.handleUpstreamResponse(message);
      return;
    }

    if (isRequestMessage(message)) {
      const requestKey = requestIdKey(message.id);
      this.pendingServerRequestIds.add(requestKey);
      this.pendingServerRequests.set(requestKey, message);
      this.broadcast(message);
      void this.pushNotifier
        ?.notifyServerRequest(message, this.buildPushNotifyOptions())
        .catch((error: unknown) => {
          console.warn(
            "Failed to send server-request push notification.",
            error,
          );
        });
      return;
    }

    this.broadcast(message);
    void this.handleUpstreamNotification(message).catch((error: unknown) => {
      console.warn("Failed to handle upstream notification.", error);
    });
  }

  private handleUpstreamResponse(
    message: JsonRpcResponseMessage | JsonRpcErrorMessage,
  ): void {
    const requestKey = requestIdKey(message.id);
    const proxyRequest = this.pendingProxyRequests.get(requestKey);
    if (proxyRequest) {
      this.pendingProxyRequests.delete(requestKey);
      if (isErrorMessage(message)) {
        proxyRequest.reject(message.error);
      } else {
        proxyRequest.resolve(message.result);
      }
      return;
    }

    const forwarded = this.forwardedBrowserRequests.get(requestKey);
    if (!forwarded) {
      return;
    }

    this.forwardedBrowserRequests.delete(requestKey);
    if (!canSend(forwarded.socket)) {
      return;
    }

    if (isResponseMessage(message)) {
      const thread = extractThreadFromResponse(message.result);
      if (thread) {
        this.cache.mergeThread(thread);
        this.broadcastThreadListSnapshot();
      }
      sendJson(forwarded.socket, {
        id: forwarded.browserRequestId,
        result: message.result,
      });
      return;
    }

    sendJson(forwarded.socket, {
      error: message.error,
      id: forwarded.browserRequestId,
    });
  }

  private async handleUpstreamNotification(
    message: JsonRpcNotificationMessage,
  ): Promise<void> {
    const notification = message as ServerNotification;

    if (notification.method === "serverRequest/resolved") {
      const requestKey = requestIdKey(notification.params.requestId);
      this.pendingServerRequestIds.delete(requestKey);
      this.pendingServerRequests.delete(requestKey);
      this.respondedServerRequestIds.delete(requestKey);
      return;
    }

    let changed = this.cache.applyNotification(notification);

    if (notification.method === "thread/unarchived") {
      changed =
        (await this.refreshThread(notification.params.threadId)) || changed;
    }

    if (changed) {
      this.broadcastThreadListSnapshot();
    }

    await this.pushNotifier?.notifyServerNotification(
      notification,
      this.buildPushNotifyOptions(),
    );
  }

  private async refreshThread(threadId: string): Promise<boolean> {
    const response = await this.requestUpstream<{ thread: Thread }>(
      "thread/read",
      {
        includeTurns: false,
        threadId,
      },
    );
    if (!response || !isObject(response) || !("thread" in response)) {
      return false;
    }

    const thread = extractThreadFromResponse(response);
    if (!thread) {
      return false;
    }

    return this.cache.mergeThread(thread);
  }

  private buildThreadListResponse(): ProxyThreadListResponse {
    const snapshot = this.cache.snapshot();
    return {
      backwardsCursor: null,
      data: snapshot.threads,
      nextCursor: null,
      previewsByThreadId: snapshot.previewsByThreadId,
    };
  }

  private broadcastThreadListSnapshot(): void {
    const notification: ProxyThreadListUpdatedNotification = {
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: this.cache.snapshot(),
    };
    this.broadcast(notification);
  }

  private sendThreadListSnapshotToSocket(socket: WebSocket): void {
    const notification: ProxyThreadListUpdatedNotification = {
      method: PROXY_THREAD_LIST_UPDATED_METHOD,
      params: this.cache.snapshot(),
    };
    sendJson(socket, notification);
  }

  private sendPendingServerRequestsToSocket(socket: WebSocket): void {
    for (const [requestKey, request] of this.pendingServerRequests) {
      if (!this.respondedServerRequestIds.has(requestKey)) {
        sendJson(socket, request);
      }
    }
  }

  private updateSocketPushSubscriptionEndpoint(
    socket: WebSocket,
    params: unknown,
  ): void {
    const endpoint = pushSubscriptionEndpointFromParams(params);
    if (endpoint) {
      this.pushSubscriptionEndpointsBySocket.set(socket, endpoint);
      return;
    }

    this.pushSubscriptionEndpointsBySocket.delete(socket);
  }

  private buildPushNotifyOptions(): PushNotifyOptions {
    return {
      connectedEndpoints: new Set(
        this.pushSubscriptionEndpointsBySocket.values(),
      ),
    };
  }

  private broadcast(message: JsonRpcMessage): void {
    for (const socket of this.browserSockets) {
      sendJson(socket, message);
    }
  }

  private sendUpstream(message: JsonRpcMessage): void {
    if (!this.upstreamSocket || !canSend(this.upstreamSocket)) {
      throw new Error("upstream websocket is not open");
    }
    this.upstreamSocket.send(JSON.stringify(message));
  }

  private async requestUpstream<TResponse>(
    method: string,
    params: unknown,
  ): Promise<TResponse> {
    if (!this.upstreamSocket || !canSend(this.upstreamSocket)) {
      throw new Error("upstream websocket is not open");
    }

    const id = `proxy:${this.nextUpstreamRequestId}`;
    this.nextUpstreamRequestId += 1;
    const requestKey = requestIdKey(id);

    const response = new Promise<unknown>((resolve, reject) => {
      this.pendingProxyRequests.set(requestKey, { resolve, reject });
    });

    this.sendUpstream({
      id,
      method,
      params,
    });

    return (await response) as TResponse;
  }

  private handleUpstreamDisconnect(code: number, reason: string): void {
    if (this.upstreamSocket) {
      this.upstreamSocket.removeAllListeners();
      closeIfOpen(this.upstreamSocket, code, reason);
      this.upstreamSocket = null;
    }
    this.initializeResponse = null;

    const disconnectError = new Error(reason || "upstream connection closed");
    for (const pending of this.pendingProxyRequests.values()) {
      pending.reject(disconnectError);
    }
    this.pendingProxyRequests.clear();
    this.forwardedBrowserRequests.clear();
    this.pendingServerRequests.clear();
    this.pendingServerRequestIds.clear();
    this.respondedServerRequestIds.clear();
    this.pushSubscriptionEndpointsBySocket.clear();

    for (const socket of this.browserSockets) {
      closeIfOpen(socket, 1011, reason || "upstream connection closed");
    }
  }
}

export type RelayOptions = {
  pushNotifier?: PushNotifier | null;
};

export async function attachRelay(
  server: HttpServer,
  backendUrl: string,
  options: RelayOptions = {},
): Promise<void> {
  const relay = new RelayController(backendUrl, options.pushNotifier ?? null);
  await relay.start();
  relay.attach(server);
}
