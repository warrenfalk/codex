#!/usr/bin/env python3

import json
import re
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = REPO_ROOT / "nix" / "bundled-shell-tools.json"
PROMPT_FILES = [
    "codex-rs/core/gpt-5.1-codex-max_prompt.md",
    "codex-rs/core/gpt-5.2-codex_prompt.md",
    "codex-rs/core/gpt_5_1_prompt.md",
    "codex-rs/core/gpt_5_2_prompt.md",
    "codex-rs/core/gpt_5_codex_prompt.md",
    "codex-rs/core/models.json",
    "codex-rs/core/prompt.md",
    "codex-rs/core/prompt_with_apply_patch_instructions.md",
    "codex-rs/core/templates/model_instructions/gpt-5.2-codex_instructions_template.md",
]
PROMPT_PREFIX = (
    "- When searching for text or files, prefer using `rg` or `rg --files` "
    "respectively because `rg` is much faster than alternatives like `grep`. "
)
PROMPT_PATTERN = re.compile(
    r"- When searching for text or files, prefer using `rg` or `rg --files` "
    r"respectively because `rg` is much faster than alternatives like `grep`\."
    r"[^\n]*?fall back gracefully to available alternatives\."
)


def main() -> None:
    manifest = json.loads(MANIFEST_PATH.read_text())
    prompt_replacement = f"{PROMPT_PREFIX}{manifest['promptBlurb']}"

    updated_files = 0
    for relative_path in PROMPT_FILES:
        file_path = REPO_ROOT / relative_path
        current = file_path.read_text()

        if not PROMPT_PATTERN.search(current):
            raise RuntimeError(f"expected bundled-shell-tools guidance in {relative_path}")

        file_path.write_text(PROMPT_PATTERN.sub(prompt_replacement, current))
        updated_files += 1

    print(f"Updated bundled shell tool guidance in {updated_files} files.")


if __name__ == "__main__":
    main()
