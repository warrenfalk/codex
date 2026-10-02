import datetime as dt
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor

import replay


SCRIPT = Path(replay.__file__).resolve()


class ReplayCommandsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.state = self.root / "state with spaces"

    def invoke(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=self.root,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_start_is_durable_before_child_and_literal_arguments_and_failure_survive(
        self,
    ) -> None:
        literal = "a b; $(not-a-command) `also-not-a-command`"
        child = (
            "import json, pathlib, sys; "
            "records = list(pathlib.Path(sys.argv[1]).glob('logs/*.json')); "
            "record = json.loads(records[0].read_text()); "
            "assert record['status'] == 'running' and record['exit_code'] is None; "
            "print(sys.argv[2]); print('child stderr', file=sys.stderr); sys.exit(7)"
        )
        command = [sys.executable, "-c", child, str(self.state), literal]
        result = self.invoke(
            "run", "--state-dir", str(self.state), "--label", "probe", "--", *command
        )
        self.assertEqual(result.returncode, 7, result.stderr)
        record = json.loads(next((self.state / "logs").glob("*.json")).read_text())
        self.assertEqual(
            {
                key: record[key]
                for key in ("status", "exit_code", "command", "cwd", "head")
            },
            {
                "status": "complete",
                "exit_code": 7,
                "command": command,
                "cwd": str(self.root.resolve()),
                "head": None,
            },
        )
        output = Path(record["log"]).read_text()
        self.assertIn(literal, output)
        self.assertIn("child stderr", output)
        self.assertGreater(record["elapsed_seconds"], 0)
        self.assertFalse(list((self.state / "logs").glob("*.tmp")))

    def test_missing_executable_leaves_a_nonpassing_launch_record(self) -> None:
        result = self.invoke(
            "run",
            "--state-dir",
            str(self.state),
            "--label",
            "missing",
            "--",
            str(self.root / "missing-tool"),
        )
        self.assertEqual(result.returncode, 127)
        summary = replay.summarize(replay.read_attempts(self.state))
        self.assertEqual(summary["completed"], 0)
        self.assertEqual(
            [item["status"] for item in summary["incomplete"]], ["launch-failed"]
        )

    def test_concurrent_attempts_with_same_label_keep_separate_logs(self) -> None:
        def launch(value: str) -> subprocess.CompletedProcess:
            return self.invoke(
                "run",
                "--state-dir",
                str(self.state),
                "--label",
                "same",
                "--",
                sys.executable,
                "-c",
                "import sys; print(sys.argv[1])",
                value,
            )

        with ThreadPoolExecutor(max_workers=2) as executor:
            results = list(executor.map(launch, ("first", "second")))
        self.assertEqual([result.returncode for result in results], [0, 0])
        records = replay.read_attempts(self.state)
        self.assertEqual(
            {record["command"][-1] for record in records}, {"first", "second"}
        )
        self.assertEqual(len({record["log"] for record in records}), 2)

    def test_queue_freezes_refs_and_messages_without_moving_head(self) -> None:
        def git(*arguments: str) -> str:
            return subprocess.check_output(
                ["git", *arguments], cwd=self.root, text=True, stderr=subprocess.DEVNULL
            ).strip()

        git("init", "--initial-branch=main")
        hooks = self.root / "empty-hooks"
        hooks.mkdir()
        git("config", "core.hooksPath", str(hooks))
        git("config", "user.name", "Replay test")
        git("config", "user.email", "replay@example.invalid")
        git("config", "commit.gpgSign", "false")
        git("commit", "--allow-empty", "-m", "base")
        base = git("rev-parse", "HEAD")
        message = "!exec cargo update --workspace --offline"
        git("commit", "--allow-empty", "-m", message)
        source = git("rev-parse", "HEAD")
        args = (
            "init",
            "--state-dir",
            str(self.state),
            "--base",
            base,
            "--source",
            "main",
            "--target",
            base,
        )
        result = self.invoke(*args)
        self.assertEqual(result.returncode, 0, result.stderr)
        captured = (self.state / "queue.json").read_text()
        queue = json.loads(captured)
        self.assertEqual(queue["entries"], [{"source": source, "message": message}])
        self.assertEqual(queue["refs"]["source"], {"ref": "main", "sha": source})
        self.assertEqual(git("rev-parse", "HEAD"), source)
        git("commit", "--allow-empty", "-m", "later work")
        self.assertNotEqual(self.invoke(*args).returncode, 0)
        self.assertEqual((self.state / "queue.json").read_text(), captured)


class ReplayTimingTest(unittest.TestCase):
    def test_overlaps_gaps_nonzero_probes_and_unfinished_attempts_are_distinct(
        self,
    ) -> None:
        start = dt.datetime(2026, 9, 20, tzinfo=dt.timezone.utc)
        records = [
            {
                "label": f"probe-{offset}",
                "command": ["just", "test"],
                "started_at": (start + dt.timedelta(seconds=offset)).isoformat(),
                "elapsed_seconds": elapsed,
                "exit_code": code,
                "status": "complete",
            }
            for offset, elapsed, code in ((0, 10, 100), (5, 10, 0), (20, 5, 0))
        ]
        unfinished = {
            "label": "interrupted-by-power-loss",
            "status": "running",
            "exit_code": None,
        }
        summary = replay.summarize([*records, unfinished])
        self.assertEqual(
            {
                key: summary[key]
                for key in (
                    "completed",
                    "nonzero",
                    "summed_seconds",
                    "covered_seconds",
                    "span_seconds",
                    "incomplete",
                )
            },
            {
                "completed": 3,
                "nonzero": 1,
                "summed_seconds": 25,
                "covered_seconds": 20,
                "span_seconds": 25,
                "incomplete": [unfinished],
            },
        )

    def test_legacy_attempts_keep_their_recorded_timing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "attempts.jsonl"
            record = {
                "label": "original",
                "command": ["nix", "develop", "-c", "just", "test"],
                "log": "/old/logs/source-original-20260920T043213.598607Z.log",
                "elapsed_seconds": 60.5,
                "exit_code": 100,
            }
            path.write_text(json.dumps(record) + "\n")
            summary = replay.summarize(replay.read_attempts(path.parent))
        self.assertEqual(
            summary["groups"], {"just test": {"count": 1, "seconds": 60.5}}
        )
        self.assertEqual(summary["covered_seconds"], 60.5)
        self.assertEqual(summary["nonzero"], 1)


if __name__ == "__main__":
    unittest.main()
