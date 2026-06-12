#!/usr/bin/env bash
set -euo pipefail
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
"$(dirname "$0")/gen-repos.sh" "$tmp"
test -d "$tmp/toy-api/.git" || { echo "FAIL: toy-api not a git repo"; exit 1; }
test -d "$tmp/toy-cli/.git" || { echo "FAIL: toy-cli not a git repo"; exit 1; }
grep -rq "BUG:" "$tmp/toy-api" || { echo "FAIL: no planted bug in toy-api"; exit 1; }
git -C "$tmp/toy-api" rev-parse HEAD >/dev/null || { echo "FAIL: no commit"; exit 1; }
echo "PASS"
