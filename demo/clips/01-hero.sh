#!/usr/bin/env bash
# Clip 1 — agent-to-agent coordination over the wsx CLI. Create a workspace from
# the dashboard's bottom drawer, put Codex beside Claude from the agents drawer,
# and have Claude delegate the fix to Codex over `wsx agent send`; Codex fixes,
# commits and reports back, Claude verifies — no human in the loop.
#
# Usage: demo/clips/01-hero.sh            # after sandbox/bootstrap.sh
#        WSX_BIN=./target/debug/wsx demo/clips/01-hero.sh
# Output: demo/out/01-hero.mp4 (capture kept in demo/out/rec-01-hero for re-renders)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/sandbox/env.sh"
WSX="$(command -v "${WSX_BIN:-wsx}")"
WSX="$(cd "$(dirname "$WSX")" && pwd)/$(basename "$WSX")"
DB="$XDG_STATE_HOME/wsx/state.db"
OUT="$ROOT/demo/out"; mkdir -p "$OUT"
# shellcheck source=/dev/null
source "$ROOT/demo/stills/scene.sh"
# shellcheck source=/dev/null
source "$ROOT/demo/stills/still.sh"
# shellcheck source=/dev/null
source "$HERE/record.sh"

# No claude.ai session URL on camera — the clip is public.
"$WSX" config set remote_control off >/dev/null
READY='(bypass permissions on|auto mode on)'

wait_sql() { # <timeout-s> <sql returning a count>
  local t=0
  until [ "$(sqlite3 "$DB" "$2" 2>/dev/null || echo 0)" -gt 0 ]; do
    [ "$t" -ge "$1" ] && { echo "timeout: $2" >&2; return 1; }
    sleep 2; t=$((t + 2))
  done
}

trap still_down EXIT
WSX_BIN="$WSX" still_up 156 40
rec_start "$OUT/rec-01-hero"
sleep 1.5

rec_caption "Spin up an isolated workspace from the dashboard's bottom drawer"
still_keys N; sleep 1.2
rec_type security-review; sleep 0.8
still_keys Enter; sleep 2
still_keys Enter
rec_speed 3; rec_wait "$READY" 120; rec_speed 1; sleep 1

rec_caption "Add any agent harness to the same workspace — here, Codex"
still_keys C-x; sleep 1.4
still_keys a; sleep 2.2
for _ in 1 2 3; do still_keys Down; sleep 1.0; done
sleep 0.8
still_keys Enter
rec_speed 3; sleep 10; rec_speed 1

rec_caption "Ask Claude to review the code and hand the fix to Codex"
CLAUDE_ID="$(in_ws toy-api security-review agent list | awk '$2=="claude"{print $1}')"
CODEX_ID="$(in_ws toy-api security-review agent list | awk '$2=="codex"{print $1}')"
sleep 1; rec_speed 4
rec_type 'Review src/auth.py for the most serious security bug. Then hand the fix to your Codex teammate using `wsx agent send codex`: give it the exact bug, the location, and a parameterized-query fix, and ask it to fix, commit, and report back with the commit hash. Then wait for its reply.'
rec_speed 1; sleep 1; still_keys Enter
rec_speed 4
wait_sql 240 "select count(*) from agent_messages where target_agent_id=$CODEX_ID and from_agent_id=$CLAUDE_ID"
rec_speed 1
rec_caption "Claude finds the SQL injection and briefs Codex over the wsx CLI"
sleep 5

rec_caption "Codex receives the full context: bug, location, and fix"
wait_sql 120 "select count(*) from agent_messages where target_agent_id=$CODEX_ID and delivered_at is not null"
still_keys C-x; sleep 1.2; still_keys w; sleep 4
# An injected message can land unsubmitted at Codex's prompt; submit it (a no-op otherwise).
still_keys Enter
rec_caption "Codex applies a parameterized query, commits, and reports back"
rec_speed 5
wait_sql 300 "select count(*) from agent_messages where target_agent_id=$CLAUDE_ID and from_agent_id=$CODEX_ID"
rec_speed 1; sleep 4

rec_caption "Claude gets the commit hash and verifies the fix — no human in the loop"
still_keys C-x; sleep 1.2; still_keys q
wait_sql 120 "select count(*) from agent_messages where target_agent_id=$CLAUDE_ID and delivered_at is not null"
rec_speed 3; sleep 25; rec_speed 1; sleep 4

rec_caption "Two agents, one workspace, one fix — end to end"
still_keys C-x; sleep 1.2; still_keys d; sleep 5
rec_caption ""
sleep 1
rec_stop
still_down; trap - EXIT

bash "$HERE/render.sh" "$OUT/rec-01-hero" "$OUT/01-hero.mp4"
