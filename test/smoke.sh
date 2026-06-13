#!/usr/bin/env bash
# Worked example + self-test of the harness: provision, create a workspace via the
# wsx CLI, and assert it appears via both `wsx workspace list` and a state.db query.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
H="$HERE/harness.sh"
export WSX_SANDBOX_ROOT="${WSX_SANDBOX_ROOT:-/tmp/wsx-test}"
trap 'bash "$H" down >/dev/null 2>&1 || true' EXIT

bash "$H" up

bash "$H" wsx workspace create toy-api --name smoke-check

# Assertion 1 — the CLI lists it.
bash "$H" wsx workspace list toy-api | grep -q smoke-check \
  || { echo "FAIL: smoke-check not in workspace list"; exit 1; }

# Assertion 2 — it landed in state.db.
bash "$H" state "SELECT name FROM workspaces;" | grep -q smoke-check \
  || { echo "FAIL: smoke-check not in state.db"; exit 1; }

echo "SMOKE PASS"
