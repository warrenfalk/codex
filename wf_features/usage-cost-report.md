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

## Pricing assumptions

- Pricing is based on an explicit standard OpenAI API price table in USD per one
  million tokens.
- Unknown models should warn instead of being silently priced as a nearby model
  family.

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

## Edge cases

- Resumed old threads with recent activity should be included in a current
  report window.
- Forked rollouts should not double count copied parent history.
- Threads with no usable name should fall back to their thread id.
- Parse errors in individual rollout files should not prevent the rest of the
  report from rendering.
- Missing pricing should produce a warning and leave raw token totals visible.

## Validation expectations

- CLI tests cover default period selection, `--last`, `--since`, `--until`, and
  JSON/table output.
- Rollout tests cover cumulative token deltas, forked-rollout de-duplication,
  archived sessions, malformed rollout lines, and missing pricing warnings.
- Cost tests cover cached input, uncached input, output, reasoning-output share,
  zero-cost formatting, and sub-cent formatting.

Original implementation commit: `cef54f5a41` (`[util] usage cost report`)
