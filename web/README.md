# Codex Web

Mobile-first web GUI for `codex app-server`.

This package is intentionally standalone:

- its own `package.json`
- its own `pnpm-lock.yaml`
- its own `flake.nix`
- no dependency on the repo root pnpm workspace

## Development

1. Start the backend app-server separately.
2. Optionally set `CODEX_WEB_BACKEND_WS_URL` if the backend is not listening on `ws://127.0.0.1:4222`.
3. Run `pnpm dev`.

The relay process listens on `CODEX_WEB_PROXY_PORT` (default `4203`).
The Vite dev server proxies `/rpc` to that relay, so the browser always talks
to the same-origin `/rpc` websocket.
The Vite dev server listens on `CODEX_WEB_UI_PORT` (default `4202`) so it can
run beside the production build with HMR enabled.
Development servers bind to all interfaces by default, so LAN devices can open
`http://<this-machine-ip>:4202/`.
The Vite dev server is configured to accept requests for `agent.warrenfalk.com`.

## Production build

1. Start the backend app-server separately.
2. Run `pnpm run build`.
3. Run `pnpm run start`.
4. Open `http://127.0.0.1:4200`.

In production, the relay process also serves the built `dist/` assets, so there
is no separate Vite server. The browser and `/rpc` websocket both use the same
origin.

## Home Screen app

Production builds include a web app manifest, app icons, and a light service
worker so the app can be added to the iOS Home Screen from Safari. Install it
from the public HTTPS origin, for example `https://agent.warrenfalk.com/`; the
client stays domain-agnostic because `/rpc` is same-origin.

The service worker is registered only in production builds. It uses network-first
caching for same-origin app-shell assets and navigation fallback, and it does not
handle `/rpc` or non-GET requests.

## Environment

- `CODEX_WEB_BACKEND_WS_URL`
  - backend websocket URL for the relay
  - defaults to `ws://127.0.0.1:4222`
- `CODEX_WEB_HOST`
  - host address for the Vite dev server and relay
  - defaults to `0.0.0.0` during development
  - defaults to `127.0.0.1` in production
- `CODEX_WEB_PROXY_PORT`
  - relay HTTP/websocket port
  - defaults to `4203` during development
  - overrides `CODEX_WEB_UI_PORT` in production when set
- `CODEX_WEB_UI_PORT`
  - Vite dev server port
  - defaults to `4202` during development
  - production static server port unless `CODEX_WEB_PROXY_PORT` is set
  - defaults to `4200` in production
