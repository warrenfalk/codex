#!/usr/bin/env bash
git fetch upstream --tags
git rebase rust-v0.98.0 main --onto rust-v0.99.0