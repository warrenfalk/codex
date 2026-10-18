#!/usr/bin/env python3
"""Check Nix server build isolation using edited copies, without compiling Rust."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def derivations(root: Path, system: str, revision: str = "isolation-a") -> dict:
    expression = f"""
      let
        flake = builtins.getFlake {json.dumps(f"path:{root}")};
        outputs = (import {root}/flake.nix).outputs (flake.inputs // {{
          self = flake // {{ shortRev = {json.dumps(revision)}; }};
        }});
        packages = outputs.packages.{system};
      in {{
        server = packages.codex-app-server.drvPath;
        frontend = packages.default.drvPath;
      }}
    """
    return json.loads(
        subprocess.check_output(
            ["nix", "eval", "--json", "--impure", "--expr", expression],
            text=True,
        )
    )


def main() -> None:
    repo = Path(__file__).resolve().parent.parent
    system = subprocess.check_output(
        ["nix", "eval", "--raw", "--impure", "--expr", "builtins.currentSystem"],
        text=True,
    )
    files = (
        subprocess.check_output(
            [
                "git",
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
                "--",
                "flake.nix",
                "flake.lock",
                "nix",
                "codex-rs",
            ],
            cwd=repo,
        )
        .decode()
        .split("\0")
    )
    with tempfile.TemporaryDirectory(prefix="codex-server-isolation-") as directory:
        root = Path(directory)
        for relative in filter(None, files):
            source = repo / relative
            if not source.exists() and not source.is_symlink():
                continue
            destination = root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination, follow_symlinks=False)

        baseline = derivations(root, system)
        cases = [
            ("codex-rs/tui/src/lib.rs", "\n// Isolation probe.\n", False),
            ("codex-rs/tui/Cargo.toml", "\n# Isolation probe.\n", False),
            ("codex-rs/cli/src/main.rs", "\n// Isolation probe.\n", False),
            ("codex-rs/app-server/src/main.rs", "\n// Isolation probe.\n", True),
            ("codex-rs/core/src/lib.rs", "\n// Isolation probe.\n", True),
            ("codex-rs/code-mode-host/src/main.rs", "\n// Isolation probe.\n", True),
        ]
        if system.endswith("-linux"):
            cases.append(
                (
                    "codex-rs/vendor/bubblewrap/bubblewrap.c",
                    "\n/* Isolation probe. */\n",
                    True,
                )
            )
        for relative, addition, changes_server in cases:
            path = root / relative
            original = path.read_text()
            try:
                path.write_text(original + addition)
                changed = derivations(root, system)
                if changed["frontend"] == baseline["frontend"]:
                    raise AssertionError(f"Frontend failed to detect {relative}")
                if (changed["server"] != baseline["server"]) != changes_server:
                    raise AssertionError(
                        f"Unexpected server invalidation for {relative}"
                    )
            finally:
                path.write_text(original)
            print(f"PASS {relative}", flush=True)

        manifest = root / "codex-rs/Cargo.toml"
        # Exercise the placeholder-version branch as well as release checkouts.
        lines = manifest.read_text().splitlines(keepends=True)
        in_package = False
        for index, line in enumerate(lines):
            if line.startswith("["):
                in_package = line.strip() == "[workspace.package]"
            elif in_package and line.startswith("version = "):
                lines[index] = 'version = "0.0.0"\n'
                break
        else:
            raise AssertionError("Workspace version not found")
        manifest.write_text("".join(lines))
        first = derivations(root, system)
        second = derivations(root, system, revision="isolation-b")
        if first["server"] != second["server"]:
            raise AssertionError(
                "Repository revision invalidated the development server"
            )
        if first["frontend"] == second["frontend"]:
            raise AssertionError(
                "Frontend development version did not include revision"
            )
        print(
            "PASS development version is independent of repository revision", flush=True
        )


if __name__ == "__main__":
    main()
