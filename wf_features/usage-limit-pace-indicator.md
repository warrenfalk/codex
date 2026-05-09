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
- The footer and configurable status line now expose the same pacing concept in
  a compact signed delta form.
- That compact delta is computed as `remaining usage % - remaining time %`, so
  a weekly window with `70%` usage left and `30%` of the week left renders as
  `weekly +40% (2d 2h)`, while `20%` left with `30%` of the week left renders
  as `weekly -10% (2d 2h)`.
- When the concrete time-left text is unavailable, the older reset-time text is
  still used as a fallback in `/status`. When pace percentage is unavailable,
  the footer/status line fall back to raw remaining percentage; when concrete
  time-left text is unavailable, they omit the parenthesized duration.
- On narrow layouts, the trailing pace or reset text wraps instead of being
  clipped into an unreadable partial value.

## Why it matters

Percent-used alone does not tell the user whether they are consuming quota too
quickly. The pace indicator adds that missing context directly to `/status`,
and the footer now gives the same signal at a glance without requiring the user
to open the full status view.

Original implementation commit: `897be7e8b0` (`tui: show usage limit pace indicator`)
