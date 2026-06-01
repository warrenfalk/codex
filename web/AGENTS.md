# Codex RC Web Project Guidelines

## Project Structure & Module Organization

This is a standalone React + TypeScript + Vite app under `web/`; do not add it
to the repository root pnpm workspace. Application code lives in `src/`:
components in `src/components/`, shared state and protocol helpers in
`src/lib/`, test setup in `src/test/`, and protocol re-exports in `src/types/`.
The relay server lives in `server/`; static assets live in `public/`.
`dist/`, `coverage/`, and `node_modules/` are generated.

## Architecture Overview

Keep the browser app stateless outside memory. The browser connects to `/rpc`,
and the relay forwards websocket frames to the app server. React components
should not open backend connections directly. Use `src/lib/backend-store.ts`
for thread-list and per-thread subscriptions.

## Build, Test, and Development Commands

Run commands from `web/` through this package's flake:

- `nix develop . -c pnpm install`: install dependencies.
- `nix develop . -c pnpm dev`: start Vite plus the local relay.
- `nix develop . -c pnpm build`: build production assets into `dist/`.
- `nix develop . -c pnpm run typecheck`: run TypeScript checks.
- `nix develop . -c pnpm test`: run Vitest with coverage.
- `nix develop . -c pnpm run format`: check Prettier formatting.
- `nix develop . -c pnpm run format:fix`: apply Prettier formatting.

If you change `react-bottom-anchored-list` or another built component consumed
by Vite, restart any running `pnpm dev` service.

## Coding Style & Naming Conventions

Use React function components and explicit exports for shared behavior. File
names use kebab-case, such as `backend-store.ts`; component exports use
PascalCase. Let Prettier control indentation and wrapping. Keep CSS
mobile-first and full-width by default.

## Testing Guidelines

Tests use Vitest, Testing Library, and jsdom. Put tests beside code as
`*.test.ts` or `*.test.tsx`. Cover store behavior, protocol handling, relay
behavior, and request-card interactions. When changing state flow, prove
duplicate React subscriptions do not duplicate backend RPCs.

## Feature Scope

Treat this web app's features separately from fork features added to the main
Codex project. The root `wf_features/` document rule does not apply here: do
not create feature documents for this subproject unless the user explicitly
asks for one.

## Security & Configuration Tips

Default ports are production UI/static plus relay `4200`, dev UI `4202`, dev
relay `4203`, and backend websocket `ws://127.0.0.1:4222`. Development binds to
`0.0.0.0`; production binds to `127.0.0.1`. Override with `CODEX_WEB_HOST`,
`CODEX_WEB_UI_PORT`, `CODEX_WEB_PROXY_PORT`, and `CODEX_WEB_BACKEND_WS_URL`. Do
not add persistence, auth, or bridge protocol complexity unless requested.
