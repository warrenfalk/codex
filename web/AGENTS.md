# Remote Control Web Guidelines

## Project Structure & Module Organization

This is a standalone React + TypeScript + Vite package; do not add it to the repository root pnpm workspace. Application code lives in `src/`: components in `src/components/`, shared state and protocol helpers in `src/lib/`, test setup in `src/test/`, and protocol type re-exports in `src/types/`. The local relay server lives in `server/`. `dist/`, `coverage/`, and `node_modules/` are generated artifacts.

## Architecture Overview

Keep the app stateless outside memory. The browser connects to the local relay at `/rpc`; the relay forwards websocket frames to the app server. React components must not open backend connections directly. Use `src/lib/backend-store.ts` for thread-list and per-thread subscriptions. `useEffect()` should only subscribe/unsubscribe to store snapshots.

## Build, Test, and Development Commands

Run commands from `web/` through the package flake:

- `nix develop . -c pnpm install`: install dependencies.
- `nix develop . -c pnpm dev`: start Vite and the relay.
- If you update `react-bottom-anchored-list` or otherwise change the component build Vite is consuming, restart any running `pnpm dev` services for this package so the UI and relay pick up the new build. If you did not start those services yourself, tell the user they need to restart them.
- `nix develop . -c pnpm build`: build production assets.
- `nix develop . -c pnpm run typecheck`: run TypeScript.
- `nix develop . -c pnpm test`: run Vitest with coverage.
- `nix develop . -c pnpm run format`: check Prettier.
- `nix develop . -c pnpm run format:fix`: apply Prettier.

## Coding Style & Naming Conventions

Use TypeScript and React function components. Prefer focused modules and explicit exported functions for shared behavior. File names use kebab-case (`backend-store.ts`); React components use PascalCase exports. Let Prettier handle indentation and wrapping. Keep mobile-first CSS full-width by default; avoid max-width desktop assumptions unless requested.

## Testing Guidelines

Tests use Vitest, Testing Library, and jsdom. Place tests next to code as `*.test.ts` or `*.test.tsx`. Cover store behavior, protocol handling, relay behavior, and user-visible request-card interactions. When changing state flow, prove duplicate React subscriptions do not duplicate backend RPCs.

## Commit & Pull Request Guidelines

Prefix commits for this project with `[web] `. Use concise, imperative commit subjects such as `[web] remove web app width cap` or `[web] refactor web app around singleton backend store`. Keep commits scoped to one behavior change. PRs should describe user-visible behavior, list validation commands, and include screenshots or short recordings for layout changes.

## Security & Configuration Tips

Default ports are production UI/static+relay `4200`, HMR dev UI `4202`, HMR dev relay `4203`, and backend websocket `ws://127.0.0.1:4222`. Override with `CODEX_WEB_UI_PORT`, `CODEX_WEB_PROXY_PORT`, and `CODEX_WEB_BACKEND_WS_URL`. Do not add persistence, auth, or bridge protocol complexity unless the task explicitly calls for it.
