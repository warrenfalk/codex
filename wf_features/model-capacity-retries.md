# Model capacity retries

When a model reports that it is at capacity, keep the turn running and retry the
same model automatically. Users should not need to send “Continue.” This applies
to agent turns, background agents, and local and remote compaction. Capacity
retries do not consume the normal transport retry budget or trigger model or
transport fallback. Other failures retain their existing policies.

Show an active retry status with the next delay. Wait 5, 10, 20, 40, then 60
seconds between attempts, retaining the one-minute cap indefinitely. Honor a
longer server retry deadline. Interrupting the turn cancels the wait.
Existing task deadlines also remain in force, including approval-review deadlines.

After five minutes of consecutive capacity failures, show one conversation
warning and request one desktop/terminal notification, then keep retrying. The
notification uses existing notification backend, focus, and enablement settings;
custom notification lists enable it with `model-capacity`. Recovery or a different
error ends that capacity episode. A later episode starts its own backoff and
five-minute timer. Historical replay must not trigger desktop notifications.

The app-server exposes `model/capacityWarning` with the thread ID, turn ID, and
warning message. This is informational and never marks the turn as failed.

Validation covers recovery on the same model and turn after HTTP, SSE, and
WebSocket overloads, retry budgets, server retry advice, cancellation, compaction,
the five-minute warning and its deduplication, and TUI notification settings and
rendered output. Use simulated time for long waits.
