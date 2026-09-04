#!/usr/bin/env python3

import json
import re
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = REPO_ROOT / "nix" / "bundled-shell-tools.json"
PROMPT_FILES = [
    "codex-rs/protocol/src/prompts/base_instructions/default.md",
    "codex-rs/core/gpt-5.1-codex-max_prompt.md",
    "codex-rs/core/gpt-5.2-codex_prompt.md",
    "codex-rs/core/gpt_5_1_prompt.md",
    "codex-rs/core/gpt_5_2_prompt.md",
    "codex-rs/core/gpt_5_codex_prompt.md",
    "codex-rs/core/tests/fixtures/prompt_with_apply_patch_instructions.md",
    "codex-rs/core/templates/model_instructions/gpt-5.2-codex_instructions_template.md",
    "codex-rs/models-manager/models.json",
    "codex-rs/models-manager/prompt.md",
]
ORCHESTRATOR_PROMPT_FILES = [
    "codex-rs/core/templates/agents/orchestrator.md",
]
PROMPT_PREFIX = (
    "- When searching for text or files, prefer using `rg` or `rg --files` "
    "respectively because `rg` is much faster than alternatives like `grep`. "
)
PROMPT_PATTERN = re.compile(
    r"- When searching for text or files, prefer using `rg` or `rg --files` "
    r"respectively because `rg` is much faster than alternatives like `grep`\."
    r"(?: \(If the `rg` command is not found, then use alternatives\.\)"
    r"|[^\n]*?fall back gracefully to available alternatives\.)"
)
ORCHESTRATOR_PROMPT_PREFIX = (
    "- Unless you are otherwise instructed, prefer using `rg` or `rg --files` "
    "respectively when searching because `rg` is much faster than alternatives like `grep`. "
)
ORCHESTRATOR_PROMPT_PATTERN = re.compile(
    r"- Unless you are otherwise instructed, prefer using `rg` or `rg --files` "
    r"respectively when searching because `rg` is much faster than alternatives like `grep`\. "
    r"(?:If the `rg` command is not found, then use alternatives\."
    r"|[^\n]*?fall back gracefully to available alternatives\.)"
)


def update_files(
    relative_paths: list[str],
    pattern: re.Pattern[str],
    replacement: str,
) -> int:
    updated_files = 0
    for relative_path in relative_paths:
        file_path = REPO_ROOT / relative_path
        current = file_path.read_text()

        if not pattern.search(current):
            raise RuntimeError(f"expected bundled-shell-tools guidance in {relative_path}")

        file_path.write_text(pattern.sub(replacement, current))
        updated_files += 1

    return updated_files


def main() -> None:
    manifest = json.loads(MANIFEST_PATH.read_text())
    prompt_replacement = f"{PROMPT_PREFIX}{manifest['promptBlurb']}"
    orchestrator_prompt_replacement = f"{ORCHESTRATOR_PROMPT_PREFIX}{manifest['promptBlurb']}"

    updated_files = update_files(PROMPT_FILES, PROMPT_PATTERN, prompt_replacement)
    updated_files += update_files(
        ORCHESTRATOR_PROMPT_FILES,
        ORCHESTRATOR_PROMPT_PATTERN,
        orchestrator_prompt_replacement,
    )

    print(f"Updated bundled shell tool guidance in {updated_files} files.")


if __name__ == "__main__":
    main()
