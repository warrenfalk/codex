import type { RequestId } from "@/types/protocol";

import { requestIdKey } from "./request-id";

export type JsonRpcRequestMessage = {
  method: string;
  id: RequestId;
  params: unknown;
};

export type JsonRpcNotificationMessage = {
  method: string;
  params?: unknown;
};

export type JsonRpcResponseMessage = {
  id: RequestId;
  result: unknown;
};

export type JsonRpcErrorShape = {
  code: number;
  message: string;
  data?: unknown;
};

export type JsonRpcErrorMessage = {
  id: RequestId;
  error: JsonRpcErrorShape;
};

export type IncomingJsonRpcMessage =
  | JsonRpcRequestMessage
  | JsonRpcNotificationMessage;

export type ConnectionStatus =
  | "idle"
  | "connecting"
  | "open"
  | "closed"
  | "error";

type PendingRequest = {
  resolve: (value: any) => void;
  reject: (reason?: unknown) => void;
};

type StatusListener = (status: ConnectionStatus, error?: string) => void;
type MessageListener = (message: IncomingJsonRpcMessage) => void;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isResponseMessage(value: unknown): value is JsonRpcResponseMessage {
  return isObject(value) && "id" in value && "result" in value;
}

function isErrorMessage(value: unknown): value is JsonRpcErrorMessage {
  return isObject(value) && "id" in value && "error" in value;
}

function isRequestMessage(value: unknown): value is JsonRpcRequestMessage {
  return isObject(value) && "id" in value && "method" in value;
}

function isNotificationMessage(
  value: unknown,
): value is JsonRpcNotificationMessage {
  return isObject(value) && "method" in value && !("id" in value);
}

export class JsonRpcConnection {
  private socket: WebSocket | null = null;
  private nextRequestId = 1;
  private pending = new Map<string, PendingRequest>();
  private messageListeners = new Set<MessageListener>();
  private statusListeners = new Set<StatusListener>();
  private status: ConnectionStatus = "idle";

  async connect(url: string): Promise<void> {
    this.emitStatus("connecting");

    await new Promise<void>((resolve, reject) => {
      const socket = new WebSocket(url);
      this.socket = socket;

      socket.addEventListener("open", () => {
        this.emitStatus("open");
        resolve();
      });

      socket.addEventListener("message", (event) => {
        if (typeof event.data !== "string") {
          this.emitStatus("error", "received non-text websocket message");
          return;
        }

        this.handleMessage(event.data);
      });

      socket.addEventListener("error", () => {
        this.emitStatus("error", "websocket transport error");
      });

      socket.addEventListener("close", (event) => {
        this.emitStatus("closed", event.reason || "connection closed");
        this.rejectPending(event.reason || "connection closed");
      });

      socket.addEventListener(
        "error",
        () => reject(new Error("websocket transport failed to connect")),
        { once: true },
      );
    });
  }

  disconnect(): void {
    this.socket?.close();
  }

  subscribe(listener: MessageListener): () => void {
    this.messageListeners.add(listener);
    return () => {
      this.messageListeners.delete(listener);
    };
  }

  subscribeStatus(listener: StatusListener): () => void {
    this.statusListeners.add(listener);
    listener(this.status);
    return () => {
      this.statusListeners.delete(listener);
    };
  }

  async request<TResponse>(
    method: string,
    params: unknown,
  ): Promise<TResponse> {
    const id = this.nextRequestId;
    this.nextRequestId += 1;
    this.send({
      method,
      id,
      params,
    });

    return new Promise<TResponse>((resolve, reject) => {
      this.pending.set(requestIdKey(id), { resolve, reject });
    });
  }

  notify(method: string, params?: unknown): void {
    this.send({
      method,
      params,
    });
  }

  respond(id: RequestId, result: unknown): void {
    this.send({
      id,
      result,
    });
  }

  private send(payload: unknown): void {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      throw new Error("websocket connection is not open");
    }

    this.socket.send(JSON.stringify(payload));
  }

  private handleMessage(rawData: string): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(rawData);
    } catch (error) {
      this.emitStatus(
        "error",
        `failed to parse JSON-RPC message: ${String(error)}`,
      );
      return;
    }

    if (isResponseMessage(parsed)) {
      const pending = this.pending.get(requestIdKey(parsed.id));
      pending?.resolve(parsed.result);
      this.pending.delete(requestIdKey(parsed.id));
      return;
    }

    if (isErrorMessage(parsed)) {
      const pending = this.pending.get(requestIdKey(parsed.id));
      pending?.reject(parsed.error);
      this.pending.delete(requestIdKey(parsed.id));
      return;
    }

    if (isRequestMessage(parsed) || isNotificationMessage(parsed)) {
      for (const listener of this.messageListeners) {
        listener(parsed);
      }
      return;
    }

    this.emitStatus("error", "received unsupported JSON-RPC payload");
  }

  private rejectPending(reason: string): void {
    for (const pending of this.pending.values()) {
      pending.reject(new Error(reason));
    }
    this.pending.clear();
  }

  private emitStatus(status: ConnectionStatus, error?: string): void {
    this.status = status;
    for (const listener of this.statusListeners) {
      listener(status, error);
    }
  }
}
