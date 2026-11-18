import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import replay
import replay_resources as resources


class ResourcePreflightTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def plan(self, **overrides):
        args = dict(
            cwd=self.root,
            paths=[],
            jobs=None,
            memory_per_job_gib=8,
            disk_required_gib=0,
        )
        args.update(overrides)
        return resources.resource_plan(**args)

    def test_memory_budget_and_cpu_limit_both_constrain_jobs(self):
        with patch.object(
            resources, "available_resources", return_value=(64, 32 * resources.GIB)
        ):
            self.assertEqual(self.plan()["jobs"], 3)
        with patch.object(
            resources, "available_resources", return_value=(2, 32 * resources.GIB)
        ):
            self.assertEqual(self.plan()["jobs"], 2)
        with patch.object(resources, "available_resources", return_value=(64, None)):
            self.assertEqual(self.plan()["jobs"], 1)

    def test_low_memory_requires_explicit_estimate_or_job_override(self):
        with patch.object(
            resources, "available_resources", return_value=(8, resources.GIB)
        ):
            with self.assertRaisesRegex(ValueError, "Available RAM"):
                self.plan()
            self.assertEqual(self.plan(jobs=1)["jobs"], 1)

    def test_disk_estimate_checks_actual_resolved_output_filesystem(self):
        with (
            patch.object(resources, "available_resources", return_value=(2, None)),
            patch.object(
                resources.shutil,
                "disk_usage",
                return_value=SimpleNamespace(free=resources.GIB),
            ),
        ):
            with self.assertRaisesRegex(ValueError, "Insufficient disk"):
                self.plan(paths=[self.root / "not-created/target"])

    def test_nested_cgroup_limits_reduce_host_resources(self):
        child = self.root / "child"
        child.mkdir()
        (child / "cpu.max").write_text("150000 100000")
        (child / "memory.max").write_text(str(12 * resources.GIB))
        (child / "memory.current").write_text(str(2 * resources.GIB))
        (self.root / "cpu.max").write_text("max 100000")
        (self.root / "memory.max").write_text(str(8 * resources.GIB))
        (self.root / "memory.current").write_text(str(3 * resources.GIB))
        with patch.object(
            resources, "cgroup_directories", return_value=[child, self.root]
        ):
            cpus, memory = resources.available_resources()
        self.assertEqual(cpus, 1)
        self.assertLessEqual(memory, 5 * resources.GIB)

    @unittest.skipUnless(os.name == "posix", "POSIX advisory locks")
    def test_lock_excludes_overlap_and_survives_logger_handle_close(self):
        with resources.heavy_lock(self.root) as descriptor:
            child = subprocess.Popen(
                [sys.executable, "-c", "import sys; sys.stdin.read()"],
                stdin=subprocess.PIPE,
                pass_fds=(descriptor,),
            )
        try:
            with self.assertRaisesRegex(ValueError, "Another heavy command"):
                with resources.heavy_lock(self.root):
                    self.fail("lock should still belong to child")
        finally:
            child.communicate(timeout=10)
        with resources.heavy_lock(self.root):
            pass

    @unittest.skipUnless(os.name == "posix", "POSIX advisory locks")
    def test_guard_records_effective_jobs_and_preserves_command_failure(self):
        script = Path(replay.__file__).resolve()
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "run",
                "--state-dir",
                str(self.root),
                "--label",
                "guarded",
                "--heavy",
                "--jobs",
                "1",
                "--",
                sys.executable,
                "-c",
                "import os,sys; print(os.environ['CARGO_BUILD_JOBS']); sys.exit(7)",
            ],
            cwd=self.root,
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 7, result.stderr)
        record = json.loads(next((self.root / "logs").glob("*.json")).read_text())
        self.assertEqual(record["resources"]["jobs"], 1)
        self.assertEqual(record["environment"]["CARGO_BUILD_JOBS"], "1")
        self.assertEqual(record["exit_code"], 7)

    @unittest.skipUnless(os.name == "posix", "POSIX executable fixture")
    def test_direct_nix_build_receives_bounded_flags_without_real_build(self):
        executable = self.root / "nix"
        executable.write_text(
            f"#!{sys.executable}\nimport json,sys\nprint(json.dumps(sys.argv[1:]))\n"
        )
        executable.chmod(0o755)
        result = subprocess.run(
            [
                sys.executable,
                str(Path(replay.__file__).resolve()),
                "run",
                "--state-dir",
                str(self.root),
                "--label",
                "nix-guard",
                "--heavy",
                "--jobs",
                "2",
                "--",
                "nix",
                "build",
                "-L",
            ],
            cwd=self.root,
            text=True,
            capture_output=True,
            env={
                **os.environ,
                "PATH": str(self.root) + os.pathsep + os.environ.get("PATH", ""),
            },
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        record = json.loads(next((self.root / "logs").glob("*.json")).read_text())
        self.assertEqual(
            record["command"], ["nix", "build", "--cores", "2", "--max-jobs", "1", "-L"]
        )


if __name__ == "__main__":
    unittest.main()
