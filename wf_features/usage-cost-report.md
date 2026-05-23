# Usage Cost Report

## What it adds

This feature adds a top-level `codex usage` command that estimates API cost from
the token-count events persisted in rollout files.

## Final behavior

- `codex usage` reports the last seven days by default.
- `--last`, `--since`, and `--until` select a different reporting window.
- `--format table` is the default human-readable view.
- `--format json` returns the same report data as structured JSON.
- The table summary shows estimated API cost, input cost, cached and uncached
  input cost, output cost, reasoning-output cost, and raw token totals.
- Per-thread rows show estimated cost columns instead of token-count
  percentages:
  - total cost
  - input cost
  - cached input cost
  - uncached input cost
  - output cost
  - reasoning output cost
- The trailing table column is `NAME (CWD)`.
  - The name is the `/rename` title when available.
  - Otherwise it falls back to the first user prompt text.
  - If neither exists, it falls back to the thread UUID.
  - CWD paths under the user's home directory are shortened with `~`.
- JSON output preserves raw token counts under `usage` and exposes estimated
  dollars under `costs`.

## How it works

The report is built from rollout JSONL files under `~/.codex/sessions` and
`~/.codex/archived_sessions`.

- File mtimes are used as a coarse prefilter, so resumed old threads with recent
  activity are still included without scanning every historical rollout.
- `EventMsg::TokenCount` is the source of token usage.
- Token events are cumulative, so the scanner computes deltas between adjacent
  token-count events.
- `TurnContext` lines provide the active model and cwd for the token event.
- `SessionMetaLine` provides the thread id, parent-fork id, model provider, and
  initial cwd.
- Thread names come from `session_index.jsonl`.
- Forked rollouts skip copied parent history before accounting usage so
  pre-fork tokens are not double counted.

The main implementation lives in:

- `codex-rs/rollout/src/usage.rs` for rollout scanning and aggregation.
- `codex-rs/rollout/src/usage_pricing.rs` for pricing lookup and cost math.
- `codex-rs/cli/src/main.rs` for command parsing and table rendering.

## Pricing assumptions

`usage_pricing.rs` contains an explicit standard OpenAI API price table in USD
per one million tokens. Keep it explicit rather than inferred by model-family
prefix, because an unknown model should warn instead of being silently priced as
the wrong model.

The cost math is:

- cached input tokens use the cached-input rate.
- uncached input tokens use the input rate.
- output tokens use the output rate.
- reasoning output tokens are shown as their share of output cost, but they are
  not added again to the total.
- total cost is input cost plus output cost.

The report intentionally does not estimate:

- Batch, Flex, or Priority API discounts or uplifts.
- data-residency pricing.
- tool, container, file-search, or other non-token fees.
- long-context pricing uplifts.
- non-OpenAI providers such as local Ollama or LM Studio models.

When pricing is missing for a model/provider, the report emits a warning and
excludes those tokens from the cost estimate while still counting them in raw
token totals.

## Rebuilding it

To rebuild this feature:

1. Add a rollout aggregation API in `codex-rs/rollout/src/usage.rs`.
2. Walk both session roots and prefilter by rollout mtime.
3. Parse rollout lines, compute `TokenUsage` deltas, and skip copied fork
   prefixes.
4. Track active `TurnContext` metadata so each usage delta is priced with the
   model active at that point.
5. Add an explicit model price table in `usage_pricing.rs`.
6. Add the top-level `usage` subcommand in `codex-rs/cli/src/main.rs`.
7. Render the table from `costs`, not from raw token counts or percentages.
8. Keep JSON output structured with both `usage` and `costs`.

Useful validation from `codex-rs/`:

```sh
nix develop --no-write-lock-file path:$PWD/.. -c cargo test -p codex-rollout usage
nix develop --no-write-lock-file path:$PWD/.. -c cargo test -p codex-cli usage
nix develop --no-write-lock-file path:$PWD/.. -c just fix -p codex-rollout
nix develop --no-write-lock-file path:$PWD/.. -c just fix -p codex-cli
```

Original implementation commit: `cef54f5a41` (`[util] usage cost report`)
