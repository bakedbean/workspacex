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

# Assertion 1 — the CLI lists it. Capture output first so a non-zero `wsx` exit is
# distinguishable from a missing match (and visible on failure).
list_out="$(bash "$H" wsx workspace list toy-api)"
grep -q smoke-check <<<"$list_out" \
  || { echo "FAIL: smoke-check not in workspace list"; echo "$list_out"; exit 1; }

# Assertion 2 — it landed in state.db.
bash "$H" state "SELECT name FROM workspaces;" | grep -q smoke-check \
  || { echo "FAIL: smoke-check not in state.db"; exit 1; }

# Assertion 3 — the TUI text-renders the new workspace (tmux capture path).
# Press 'l' first to expand the focused repo (idle repos are folded by default).
shot_txt="$(mktemp)"
bash "$H" capture l "$shot_txt"
grep -q smoke-check "$shot_txt" \
  || { echo "FAIL: smoke-check not visible in TUI capture"; cat "$shot_txt"; rm -f "$shot_txt"; exit 1; }
rm -f "$shot_txt"

echo "SMOKE PASS"
