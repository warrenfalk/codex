export type PushNotificationState =
  | { type: "unsupported" }
  | { type: "permissionDenied" }
  | { type: "disabled" }
  | { type: "enabled"; endpoint: string }
  | { type: "error"; message: string };

type PublicKeyResponse = {
  publicKey: string;
};

function isPushSupported(): boolean {
  return (
    window.isSecureContext &&
    "Notification" in window &&
    "PushManager" in window &&
    "serviceWorker" in navigator
  );
}

function urlBase64ToUint8Array(value: string): Uint8Array<ArrayBuffer> {
  const padding = "=".repeat((4 - (value.length % 4)) % 4);
  const base64 = `${value}${padding}`.replace(/-/g, "+").replace(/_/g, "/");
  const raw = window.atob(base64);
  const output = new Uint8Array(raw.length);

  for (let index = 0; index < raw.length; index += 1) {
    output[index] = raw.charCodeAt(index);
  }

  return output;
}

async function fetchPublicKey(): Promise<string> {
  const response = await fetch("/api/push/vapid-public-key");
  if (!response.ok) {
    throw new Error("Could not load notification key.");
  }

  const body = (await response.json()) as PublicKeyResponse;
  if (typeof body.publicKey !== "string" || body.publicKey.length === 0) {
    throw new Error("Notification key response was invalid.");
  }

  return body.publicKey;
}

async function getServiceWorkerRegistration(): Promise<ServiceWorkerRegistration> {
  const existing = await navigator.serviceWorker.getRegistration("/");
  if (existing) {
    return existing;
  }

  return await navigator.serviceWorker.register("/sw.js", { scope: "/" });
}

async function getExistingSubscription(): Promise<PushSubscription | null> {
  if (!isPushSupported() || Notification.permission !== "granted") {
    return null;
  }

  const registration = await getServiceWorkerRegistration();
  return await registration.pushManager.getSubscription();
}

async function saveSubscription(subscription: PushSubscription): Promise<void> {
  const response = await fetch("/api/push/subscriptions", {
    body: JSON.stringify({ subscription: subscription.toJSON() }),
    headers: {
      "content-type": "application/json",
    },
    method: "POST",
  });

  if (!response.ok) {
    throw new Error("Could not save notification subscription.");
  }
}

export async function getPushSubscriptionEndpoint(): Promise<string | null> {
  return (await getExistingSubscription())?.endpoint ?? null;
}

export async function readPushNotificationState(): Promise<PushNotificationState> {
  if (!isPushSupported()) {
    return { type: "unsupported" };
  }

  if (Notification.permission === "denied") {
    return { type: "permissionDenied" };
  }

  const subscription = await getExistingSubscription();
  if (subscription) {
    await saveSubscription(subscription);
    return { type: "enabled", endpoint: subscription.endpoint };
  }

  return { type: "disabled" };
}

export async function enablePushNotifications(): Promise<PushNotificationState> {
  if (!isPushSupported()) {
    return { type: "unsupported" };
  }

  const permission = await Notification.requestPermission();
  if (permission === "denied") {
    return { type: "permissionDenied" };
  }

  if (permission !== "granted") {
    return { type: "disabled" };
  }

  const registration = await getServiceWorkerRegistration();
  const existing = await registration.pushManager.getSubscription();
  if (existing) {
    await saveSubscription(existing);
    return { type: "enabled", endpoint: existing.endpoint };
  }

  const publicKey = await fetchPublicKey();
  const subscription = await registration.pushManager.subscribe({
    applicationServerKey: urlBase64ToUint8Array(publicKey),
    userVisibleOnly: true,
  });
  await saveSubscription(subscription);
  return { type: "enabled", endpoint: subscription.endpoint };
}

export async function disablePushNotifications(): Promise<PushNotificationState> {
  const subscription = await getExistingSubscription();
  if (!subscription) {
    return await readPushNotificationState();
  }

  const response = await fetch("/api/push/subscriptions", {
    body: JSON.stringify({ endpoint: subscription.endpoint }),
    headers: {
      "content-type": "application/json",
    },
    method: "DELETE",
  });
  if (!response.ok) {
    throw new Error("Could not remove notification subscription.");
  }

  await subscription.unsubscribe();
  return await readPushNotificationState();
}
