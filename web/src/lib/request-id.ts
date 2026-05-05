import type { RequestId } from "@/types/protocol";

export function requestIdKey(id: RequestId): string {
  return `${typeof id}:${String(id)}`;
}
