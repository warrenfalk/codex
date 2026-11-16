# Usage Limit Pace Indicator

## What it adds

This feature upgrades the TUI status card so usage windows show pacing, not
just raw percent used.

## Final behavior

- Rate-limit rows now carry both quota usage and time-left information.
- The progress bar can show a time marker in addition to the remaining-quota
  bar, so the user can see whether they are ahead of or behind the reset pace.
- The status text can include phrases like `60% time left` and concrete time
  remaining before reset.
- The configurable `weekly-limit` status-line item shows two whole-number
  percentages, usage quota remaining first and time remaining second:
  `limit 94%, time 97%`. Time remaining is the fraction of the usage window
  before its reset; usage remaining is `100% - usage consumed`. Both values
  are clamped to `0%` through `100%`. The terminal-title item with the same name
  uses the same text.
- The separate `weekly-limit-bar` status-line item shows the same bracketed,
  20-cell bar as `/status`: `█` is quota remaining, `░` is quota consumed, and
  `│` marks the percentage of time remaining. It can be selected independently
  or alongside `weekly-limit` in `/statusline`, including its live preview.
- Both weekly items prefer the actual weekly window regardless of whether it
  arrives as the primary or secondary window. They retain the secondary-window
  fallback when no weekly window exists; the percentage text then identifies
  that window, such as `monthly limit 65%, time 80%`.
- The five-hour status-line item retains its compact signed pace delta and
  reset countdown.
- When the concrete time-left text is unavailable, the older reset-time text is
  still used as a fallback in `/status`. Without time-percentage data,
  `weekly-limit` shows only the remaining quota, such as `limit 94%`, and
  `weekly-limit-bar` omits its time marker. Both items are omitted when their
  usage window is unavailable.
- On narrow layouts, the trailing pace or reset text wraps instead of being
  clipped into an unreadable partial value.

## Why it matters

Percent-used alone does not tell the user whether they are consuming quota too
quickly. The pace indicator adds that missing context directly to `/status`,
and the footer now gives the same signal at a glance without requiring the user
to open the full status view.

## Validation expectations

- Cover independent quota/time percentages, missing reset data, boundary
  clamping, weekly windows in either position, and unavailable usage windows.
- Snapshot the percentage footer, the bar alone and alongside percentages,
  a narrow footer, and the status-line picker with both weekly items selected.

Original implementation commit: `897be7e8b0` (`tui: show usage limit pace indicator`)
