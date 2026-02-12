#!/usr/bin/env bash
git fetch upstream --tags --quiet
BASE_TAG=$(git describe --tags --abbrev=0)
LATEST_TAG=$(curl -s "https://api.github.com/repos/openai/codex/releases/latest" | jq -r '.tag_name')
if [ "$BASE_TAG" = "$LATEST_TAG" ]; then
  echo "Already up to date with ${LATEST_TAG}"
  exit 0
fi
echo "Rebasing from ${BASE_TAG} to main onto ${LATEST_TAG}"
git rebase "${BASE_TAG}" main --onto "${LATEST_TAG}"