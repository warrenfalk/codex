"""Opt-in resource preflight for replay builds; no third-party dependencies."""

from contextlib import contextmanager
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

GIB = 1024**3


def cgroup_directories():
    """Inspect the current Linux cgroup v2 and its containing limits."""
    try:
        relative = next(
            line[3:]
            for line in Path("/proc/self/cgroup").read_text().splitlines()
            if line.startswith("0::")
        )
    except (OSError, StopIteration):
        return []
    root = Path("/sys/fs/cgroup")
    current = root / relative.lstrip("/")
    if ".." in current.parts:
        return []
    return [current, *[p for p in current.parents if p == root or root in p.parents]]


def available_resources():
    """Use available memory and CPU limits, falling back to serial compilation."""
    cpus = os.cpu_count() or 1
    if hasattr(os, "sched_getaffinity"):
        cpus = min(cpus, len(os.sched_getaffinity(0)))
    memory = None
    try:
        fields = dict(
            line.split(":", 1)
            for line in Path("/proc/meminfo").read_text().splitlines()
        )
        memory = int(fields["MemAvailable"].split()[0]) * 1024
    except (OSError, KeyError, ValueError):
        # macOS: count immediately free and inactive pages, not total installed RAM.
        try:
            output = subprocess.check_output(
                ["vm_stat"], text=True, stderr=subprocess.DEVNULL
            )
            page_size = int(output.split("page size of ")[1].split()[0])
            pages = sum(
                int(line.split(":")[1].strip().rstrip("."))
                for line in output.splitlines()
                if line.startswith(("Pages free:", "Pages inactive:"))
            )
            memory = pages * page_size
        except (OSError, ValueError, IndexError, subprocess.CalledProcessError):
            pass
    for directory in cgroup_directories():
        try:
            quota, period = (directory / "cpu.max").read_text().split()
            if quota != "max":
                cpus = min(cpus, max(1, int(quota) // int(period)))
        except (OSError, ValueError):
            pass
        try:
            limit = (directory / "memory.max").read_text().strip()
            if limit != "max":
                remaining = max(
                    0, int(limit) - int((directory / "memory.current").read_text())
                )
                memory = remaining if memory is None else min(memory, remaining)
        except (OSError, ValueError):
            pass
    return max(1, cpus), memory


def resource_plan(cwd, paths, jobs, memory_per_job_gib, disk_required_gib):
    if memory_per_job_gib <= 0 or not math.isfinite(memory_per_job_gib):
        raise ValueError("--memory-per-job-gib must be finite and positive")
    if disk_required_gib < 0 or not math.isfinite(disk_required_gib):
        raise ValueError("--disk-required-gib must be finite and nonnegative")
    if jobs is not None and jobs < 1:
        raise ValueError("--jobs must be positive")
    cpus, memory = available_resources()
    if jobs is None and memory is not None and memory * 0.75 < memory_per_job_gib * GIB:
        raise ValueError(
            "Available RAM is below the one-job estimate plus host reserve; "
            "free memory, adjust --memory-per-job-gib, or explicitly override --jobs"
        )
    # Reserve 25% of currently available RAM for the host and other processes.
    automatic = (
        min(cpus, max(1, int(memory * 0.75 / (memory_per_job_gib * GIB))))
        if memory is not None
        else 1
    )
    selected = jobs if jobs is not None else automatic
    default_target = (
        cwd / "target" if (cwd / "Cargo.toml").exists() else cwd / "codex-rs/target"
    )
    target = Path(os.environ.get("CARGO_TARGET_DIR", str(default_target)))
    candidates = [cwd, target, Path(tempfile.gettempdir()), *paths]
    if Path("/nix/store").exists():
        candidates.append(Path("/nix/store"))
    disks = []
    seen = set()
    for candidate in candidates:
        path = (cwd / candidate).resolve()
        while not path.exists():
            path = path.parent
        device = path.stat().st_dev
        if device in seen:
            continue
        seen.add(device)
        usage = shutil.disk_usage(path)
        # Two GiB is emergency headroom, not an estimate of a complete build.
        required = (disk_required_gib + 2) * GIB
        if usage.free < required:
            raise ValueError(
                f"Insufficient disk at {path}: {usage.free / GIB:.1f} GiB free; "
                f"need {required / GIB:.1f} GiB including reserve"
            )
        disks.append(
            {"path": str(path), "free_bytes": usage.free, "required_bytes": required}
        )
    return {
        "cpu_limit": cpus,
        "available_memory_bytes": memory,
        "jobs": selected,
        "memory_per_job_gib": memory_per_job_gib,
        "jobs_overridden": jobs is not None,
        "disks": disks,
    }


@contextmanager
def heavy_lock(state_dir):
    """Lease the lock to a supervisor if the logger exits early."""
    try:
        import fcntl
    except ImportError as error:
        raise ValueError("--heavy requires POSIX file locking (Linux/macOS)") from error
    state_dir.mkdir(parents=True, exist_ok=True)
    with (state_dir / "heavy.lock").open("a+") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError(
                "Another heavy command holds this replay's lock; wait for it to finish"
            ) from error
        # Do not explicitly unlock: a surviving child must retain the lock.
        yield handle.fileno()


def supervised_command(command):
    """Hold the inherited lease without exposing it to the workload or helpers."""
    return [sys.executable, str(Path(__file__).resolve()), *command]


if __name__ == "__main__":
    # Popen closes the inherited lock descriptor in the actual workload. Only
    # this supervisor retains it, so background helpers cannot prolong the lease.
    try:
        workload = subprocess.Popen(sys.argv[1:], close_fds=True)
    except OSError as error:
        print(f"Could not launch command: {error}", file=sys.stderr)
        sys.exit(127)
    while True:
        try:
            status = workload.wait()
            break
        except KeyboardInterrupt:
            # Keep the lease until the workload actually exits, even when the
            # logger is interrupted. Do not terminate a running compiler.
            continue
    sys.exit(status if status >= 0 else 128 - status)
