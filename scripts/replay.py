#!/usr/bin/env python3
"""Capture a release queue, log replay commands, and report their elapsed time."""

import argparse
import datetime as dt
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import time
import uuid


def write_record(path: Path, record: dict) -> None:
    """Replace a complete record atomically and flush it before returning."""
    temporary = path.with_suffix(".tmp")
    with temporary.open("w", encoding="utf-8") as handle:
        json.dump(record, handle, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)
    if os.name == "posix":
        descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)


def git(cwd: Path, *arguments: str) -> str:
    return subprocess.check_output(
        ["git", *arguments], cwd=cwd, text=True, stderr=subprocess.PIPE
    ).strip()


def init_queue(state_dir: Path, cwd: Path, base: str, source: str, target: str) -> None:
    path = state_dir / "queue.json"
    if path.exists():
        raise ValueError(f"Queue already exists; preserve the captured IDs: {path}")
    refs = {
        key: {"ref": ref, "sha": git(cwd, "rev-parse", "--verify", f"{ref}^{{commit}}")}
        for key, ref in (("base", base), ("source", source), ("target", target))
    }
    git(cwd, "merge-base", "--is-ancestor", refs["base"]["sha"], refs["source"]["sha"])
    source_range = f"{refs['base']['sha']}..{refs['source']['sha']}"
    fields = git(cwd, "log", "--reverse", "--format=%H%x00%B%x00", source_range).split(
        "\0"
    )
    entries = [
        {"source": sha.strip(), "message": message.rstrip()}
        for sha, message in zip(fields[::2], fields[1::2])
    ]
    state_dir.mkdir(parents=True, exist_ok=True)
    write_record(path, {"refs": refs, "entries": entries})
    print(f"Captured {len(entries)} source commits: {path}")


def tail(path: Path) -> str:
    with path.open("rb") as handle:
        handle.seek(0, os.SEEK_END)
        handle.seek(max(0, handle.tell() - 8192))
        return "\n".join(
            line[:500]
            for line in handle.read().decode(errors="replace").splitlines()[-12:]
        )


def run_command(state_dir: Path, label: str, cwd: Path, command: list[str]) -> int:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,119}", label):
        raise ValueError(
            "Use a label of 1-120 letters, digits, dots, hyphens or underscores"
        )
    if not command:
        raise ValueError("A command is required after --")
    cwd = cwd.resolve(strict=True)
    logs = state_dir.resolve() / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    started = dt.datetime.now(dt.timezone.utc)
    name = f"{started.strftime('%Y%m%dT%H%M%S.%fZ')}-{label}-{uuid.uuid4().hex[:8]}"
    log = logs / f"{name}.log"
    metadata = logs / f"{name}.json"
    try:
        head = git(cwd, "rev-parse", "HEAD")
        changes = git(cwd, "status", "--porcelain=v1")
    except subprocess.CalledProcessError:
        head, changes = None, None
    environment_keys = (
        "CARGO_HOME",
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_JOBS",
        "RUSTFLAGS",
        "RUSTUP_TOOLCHAIN",
        "RUSTY_V8_ARCHIVE",
        "RUSTY_V8_SRC_BINDING_PATH",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER",
        "RUST_MIN_STACK",
        "NEXTEST_PROFILE",
        "TMPDIR",
        "TMP",
        "TEMP",
        "TEMPDIR",
        "NIX_BUILD_TOP",
        "NO_COLOR",
    )
    record = {
        "label": label,
        "cwd": str(cwd),
        "command": command,
        "started_at": started.isoformat(),
        "head": head,
        "git_status": changes,
        "environment": {
            key: os.environ[key] for key in environment_keys if key in os.environ
        },
        "log": str(log),
        "status": "running",
        "exit_code": None,
    }
    start = time.monotonic()
    with log.open("w", encoding="utf-8") as handle:
        handle.write(f"cwd: {cwd}\ncommand: {shlex.join(command)}\nhead: {head}\n\n")
        handle.flush()
        os.fsync(handle.fileno())
        write_record(metadata, record)
        print(f"Running {label}; log {log}", flush=True)
        process = None
        try:
            process = subprocess.Popen(
                command, cwd=cwd, stdout=handle, stderr=subprocess.STDOUT
            )
        except OSError as error:
            handle.write(f"Could not launch command: {error}\n")
            record.update(status="launch-failed", exit_code=127)
        if process is not None:
            record["pid"] = process.pid
            write_record(metadata, record)
            try:
                while True:
                    try:
                        record["exit_code"] = process.wait(timeout=45)
                        record["status"] = "complete"
                        break
                    except subprocess.TimeoutExpired:
                        os.fsync(handle.fileno())
                        print(
                            f"Still running {label}: {time.monotonic() - start:.0f}s; log {log}",
                            flush=True,
                        )
            except KeyboardInterrupt:
                # No implicit kill/restart of a potentially active Rust command.
                record["status"] = "interrupted"
        handle.flush()
        os.fsync(handle.fileno())
    record.update(
        ended_at=dt.datetime.now(dt.timezone.utc).isoformat(),
        elapsed_seconds=round(time.monotonic() - start, 3),
    )
    write_record(metadata, record)
    print(
        f"{label}: {record['status']}; exit {record['exit_code']}; {record['elapsed_seconds']}s"
    )
    print(tail(log))
    print(f"Record: {metadata}")
    code = record["exit_code"]
    return 130 if code is None else (128 - code if code < 0 else code)


def read_attempts(path: Path) -> list[dict]:
    if path.is_file():
        return [
            json.loads(line) for line in path.read_text().splitlines() if line.strip()
        ]
    records = [
        json.loads(file.read_text()) for file in sorted((path / "logs").glob("*.json"))
    ]
    legacy = path / "attempts.jsonl"
    if legacy.exists():
        records.extend(read_attempts(legacy))
    return records


def command_group(command: list[str]) -> str:
    pairs = list(zip(command, command[1:]))
    for pair in (
        ("just", "test"),
        ("nix", "build"),
        ("cargo", "build"),
        ("just", "fmt"),
        ("just", "fix"),
    ):
        if pair in pairs:
            return " ".join(pair)
    return "other"


def summarize(records: list[dict]) -> dict:
    intervals = []
    groups = {}
    completed = []
    incomplete = []
    for record in records:
        if (
            record.get("status", "complete") != "complete"
            or record.get("exit_code") is None
        ):
            incomplete.append(record)
            continue
        elapsed = float(record["elapsed_seconds"])
        if "started_at" in record:
            started = dt.datetime.fromisoformat(
                record["started_at"].replace("Z", "+00:00")
            )
        else:
            stamp = re.search(r"(\d{8}T\d{6}\.\d+Z)\.log$", record["log"])
            if stamp is None:
                raise ValueError(f"Missing start time for {record['log']}")
            started = dt.datetime.strptime(stamp[1], "%Y%m%dT%H%M%S.%fZ").replace(
                tzinfo=dt.timezone.utc
            )
        intervals.append((started, started + dt.timedelta(seconds=elapsed)))
        group = groups.setdefault(
            command_group(record["command"]), {"count": 0, "seconds": 0}
        )
        group["count"] += 1
        group["seconds"] += elapsed
        completed.append(record)
    merged = []
    for start, end in sorted(intervals):
        if merged and start <= merged[-1][1]:
            merged[-1][1] = max(end, merged[-1][1])
        else:
            merged.append([start, end])
    return {
        "completed": len(completed),
        "nonzero": sum(record["exit_code"] != 0 for record in completed),
        "incomplete": incomplete,
        "summed_seconds": sum(record["elapsed_seconds"] for record in completed),
        "covered_seconds": sum((end - start).total_seconds() for start, end in merged),
        "span_seconds": (merged[-1][1] - merged[0][0]).total_seconds() if merged else 0,
        "groups": groups,
        "longest": sorted(
            completed, key=lambda record: record["elapsed_seconds"], reverse=True
        )[:10],
    }


def report(path: Path) -> None:
    if not path.exists():
        raise ValueError(f"No such replay state or attempt log: {path}")
    summary = summarize(read_attempts(path))
    print(
        f"Completed commands: {summary['completed']}; nonzero exits: {summary['nonzero']}"
    )
    print(
        f"Incomplete/interrupted/launch-failed attempts: {len(summary['incomplete'])}"
    )
    for label, key in (
        ("Summed command time", "summed_seconds"),
        ("Wall time covered by commands", "covered_seconds"),
        ("Recorded command span including gaps", "span_seconds"),
    ):
        print(f"{label}: {summary[key] / 60:.1f} minutes")
    print(
        "Overlapping commands count once in covered wall time. Missing completions are excluded."
    )
    print(
        "Nonzero exits include intentional bug probes; classification belongs in the ledger."
    )
    for group, totals in summary["groups"].items():
        print(
            f"  {group}: {totals['count']} commands, {totals['seconds'] / 60:.1f} summed minutes"
        )
    print("Longest completed commands:")
    for record in summary["longest"]:
        print(
            f"  {record['elapsed_seconds'] / 60:.1f}m exit={record['exit_code']} {record['label']} ({record['log']})"
        )
    for record in summary["incomplete"]:
        print(f"  Unconfirmed: {record['label']} ({record['log']})")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    init = actions.add_parser(
        "init", help="capture immutable source IDs and messages without changing Git"
    )
    init.add_argument("--state-dir", type=Path, required=True)
    init.add_argument("--cwd", type=Path, default=Path.cwd())
    for ref in ("base", "source", "target"):
        init.add_argument(f"--{ref}", required=True)
    run = actions.add_parser(
        "run", help="log a literal command and return its exit status"
    )
    run.add_argument("--state-dir", type=Path, required=True)
    run.add_argument("--label", required=True)
    run.add_argument("--cwd", type=Path, default=Path.cwd())
    run.add_argument("command", nargs=argparse.REMAINDER)
    timing = actions.add_parser(
        "report", help="summarize a state directory or legacy attempts.jsonl"
    )
    timing.add_argument("path", type=Path)
    args = parser.parse_args()
    try:
        if args.action == "init":
            init_queue(args.state_dir, args.cwd, args.base, args.source, args.target)
        elif args.action == "run":
            command = args.command[1:] if args.command[:1] == ["--"] else args.command
            return run_command(args.state_dir, args.label, args.cwd, command)
        else:
            report(args.path)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
