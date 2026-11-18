# Resource safeguards for release replay commands

Release updates can launch expensive compilation and packaging on different
machines. An opt-in guarded command mode reduces accidental overlap and resource
exhaustion without assuming a particular workstation's RAM, CPU count, or paths.

- Heavy commands sharing a replay state directory cannot overlap on supported
  Linux/macOS hosts. Contention fails promptly with an actionable explanation.
  Ordinary logged commands remain compatible and can run concurrently.
- A running child retains exclusion if its logger exits. Completed commands
  release exclusion without deleting stale PID files or requiring manual cleanup.
  Workloads and their background helpers do not inherit the lock descriptor;
  background helpers surviving a completed command cannot prolong its lease.
- Compiler parallelism uses available memory and CPU constraints, including
  available Linux cgroup v2 ancestor limits. Unknown memory falls back to serial
  compilation. Insufficient estimated one-job memory stops preflight.
- Users can override compiler jobs and estimated memory per job. Direct local
  Nix builds also serialize derivations. Estimates are not hard memory limits,
  and test concurrency and remote builders require separate consideration.
- Check actual resolved output filesystems, including additional user-specified
  locations. Enforce emergency headroom plus any explicitly estimated additional
  build space. Do not claim emergency headroom proves a full build will fit.
- Persist selected concurrency and measured resource headroom alongside command
  results. Preserve literal argument handling, durable logs, command exit status,
  and the distinction between interrupted, failed, and passed attempts.
- No system configuration changes, automatic cache retention, global process
  termination, or background supervision are introduced. Unsupported guarded
  platforms report their limitation while ordinary logging remains available.

Validation must exercise constrained memory/CPU selection, unknown-memory
fallback, low disk rejection, lock contention and lease survival without workload
descriptor inheritance, and a guarded
child failure retaining its actual exit status and recorded effective settings.
