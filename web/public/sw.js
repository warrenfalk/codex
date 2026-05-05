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
