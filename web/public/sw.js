const CACHE_NAME = "codex-web-shell-v1";
const CACHE_PREFIX = "codex-web-shell-";
const PRECACHE_URLS = [
  "/",
  "/manifest.webmanifest",
  "/icon.svg",
  "/apple-touch-icon.png",
  "/icons/icon-192.png",
  "/icons/icon-512.png",
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(PRECACHE_URLS)),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((names) =>
      Promise.all(
        names
          .filter((name) => name.startsWith(CACHE_PREFIX))
          .filter((name) => name !== CACHE_NAME)
          .map((name) => caches.delete(name)),
      ),
    ),
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  const url = new URL(request.url);

  if (
    request.method !== "GET" ||
    url.origin !== self.location.origin ||
    url.pathname === "/rpc"
  ) {
    return;
  }

  if (request.mode === "navigate") {
    event.respondWith(networkFirst(request, "/"));
    return;
  }

  if (
    ["font", "image", "manifest", "script", "style", "worker"].includes(
      request.destination,
    )
  ) {
    event.respondWith(networkFirst(request));
  }
});

self.addEventListener("push", (event) => {
  const payload = parsePushPayload(event.data);

  event.waitUntil(
    self.registration.showNotification(payload.title, {
      badge: "/icons/icon-192.png",
      body: payload.body,
      data: {
        url: payload.url,
      },
      icon: "/icons/icon-192.png",
      tag: payload.tag,
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const targetUrl = sameOriginUrl(event.notification.data?.url);

  event.waitUntil(openOrFocusWindow(targetUrl));
});

self.addEventListener("pushsubscriptionchange", (event) => {
  event.waitUntil(refreshPushSubscription());
});

async function networkFirst(request, fallbackUrl) {
  const cache = await caches.open(CACHE_NAME);

  try {
    const response = await fetch(request);
    if (response.ok) {
      await cache.put(request, response.clone());
    }
    return response;
  } catch (error) {
    const cached = await cache.match(request);
    if (cached) {
      return cached;
    }

    if (fallbackUrl) {
      const fallback = await cache.match(fallbackUrl);
      if (fallback) {
        return fallback;
      }
    }

    throw error;
  }
}

function parsePushPayload(data) {
  if (!data) {
    return {
      body: "Codex needs attention.",
      tag: "codex-web",
      title: "Codex",
      url: "/",
    };
  }

  try {
    const parsed = data.json();
    return {
      body:
        typeof parsed.body === "string"
          ? parsed.body
          : "Codex needs attention.",
      tag: typeof parsed.tag === "string" ? parsed.tag : "codex-web",
      title: typeof parsed.title === "string" ? parsed.title : "Codex",
      url: sameOriginUrl(parsed.url),
    };
  } catch {
    return {
      body: data.text(),
      tag: "codex-web",
      title: "Codex",
      url: "/",
    };
  }
}

function sameOriginUrl(value) {
  try {
    const url = new URL(
      typeof value === "string" ? value : "/",
      self.location.origin,
    );
    return url.origin === self.location.origin ? url.href : "/";
  } catch {
    return "/";
  }
}

async function openOrFocusWindow(targetUrl) {
  const windowClients = await clients.matchAll({
    includeUncontrolled: true,
    type: "window",
  });

  for (const client of windowClients) {
    if (new URL(client.url).origin !== self.location.origin) {
      continue;
    }

    if ("navigate" in client) {
      await client.navigate(targetUrl);
    }
    return await client.focus();
  }

  if (clients.openWindow) {
    return await clients.openWindow(targetUrl);
  }
}

async function refreshPushSubscription() {
  const keyResponse = await fetch("/api/push/vapid-public-key");
  if (!keyResponse.ok) {
    return;
  }

  const { publicKey } = await keyResponse.json();
  if (typeof publicKey !== "string") {
    return;
  }

  const subscription = await self.registration.pushManager.subscribe({
    applicationServerKey: urlBase64ToUint8Array(publicKey),
    userVisibleOnly: true,
  });

  await fetch("/api/push/subscriptions", {
    body: JSON.stringify({ subscription: subscription.toJSON() }),
    headers: {
      "content-type": "application/json",
    },
    method: "POST",
  });
}

function urlBase64ToUint8Array(value) {
  const padding = "=".repeat((4 - (value.length % 4)) % 4);
  const base64 = `${value}${padding}`.replace(/-/g, "+").replace(/_/g, "/");
  const raw = self.atob(base64);
  const output = new Uint8Array(raw.length);

  for (let index = 0; index < raw.length; index += 1) {
    output[index] = raw.charCodeAt(index);
  }

  return output;
}
