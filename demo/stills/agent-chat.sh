#!/usr/bin/env bash
# Stage an agent-to-agent coordination scene and screenshot the ATTACHED view.
#
# One workspace (toy-api/security-review) with Claude as primary and a peer
# attached — Codex by default, or another Claude with PEER=claude (its label is
# then claude#2). Claude reviews the planted SQL injection and hands the fix to
# the peer over `wsx agent send`; the peer fixes, commits and reports back;
# Claude verifies.
# The stills show the live chat pane with the pinned-command chips
# (/pr /rebase /feedback /agent-review) and the footer agents row.
#
# Progress is tracked through wsx's own agent_messages table rather than fixed
# sleeps, so a take waits exactly as long as the agents do.
#
# Needs tmux, Chrome/Chromium, sqlite3 and logged-in `claude` + `codex` CLIs.
#
# Usage: demo/stills/agent-chat.sh           # after sandbox/bootstrap.sh
#        WSX_BIN=./target/debug/wsx demo/stills/agent-chat.sh
#        PEER=claude demo/stills/agent-chat.sh  # second Claude instead of Codex
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/sandbox/env.sh"
WSX="$(command -v "${WSX_BIN:-wsx}")"
DB="$XDG_STATE_HOME/wsx/state.db"
OUT="$ROOT/demo/out"; mkdir -p "$OUT"
PEER="${PEER:-codex}"                       # kind of the attached teammate
PEER_LABEL="$PEER"; [ "$PEER" = claude ] && PEER_LABEL="claude#2"

in_ws() { local path; path="$("$WSX" workspace path "$1" "$2")"; (cd "$path" && "$WSX" "${@:3}"); }
# wait_sql <label> <timeout-s> <sql returning a count>: poll until the count is > 0.
wait_sql() {
  local label="$1" timeout="$2" sql="$3" t=0 n
  while :; do
    n="$(sqlite3 "$DB" "$sql" 2>/dev/null || echo 0)"
    [ "${n:-0}" -gt 0 ] && { echo "  $label after ${t}s"; return 0; }
    [ "$t" -ge "$timeout" ] && { echo "  timeout waiting for $label (${timeout}s)"; return 1; }
    sleep 2; t=$((t + 2))
  done
}

# No claude.ai session URL in the banner — a still is public.
"$WSX" config set remote_control off >/dev/null

# --- pinned commands: the chips under the chat pane ---
"$WSX" config set pinned_commands "$(printf '%s\n' \
  '/pr=/pull-request' '/rebase=/rebase-on-main' '/feedback=/incorporate-feedback' '/agent-review=/agent-review')" >/dev/null

# --- the workspace: Claude primary, Codex attached ---
"$WSX" workspace create toy-api --name security-review --yolo --agent claude --prompt \
  "Review src/auth.py and name the single most serious security bug, with the line. Then hand the fix to your teammate with \`wsx agent send $PEER_LABEL\`: give it the exact bug, its location, and a parameterized-query fix, and ask it to fix, commit, and report back to you with \`wsx agent send claude\` including the commit hash. Then wait for the reply and, once it arrives, verify the fix with git show and summarize. Set your wsx recap and status as you go. Ask me no questions." >/dev/null
in_ws toy-api security-review agent add "$PEER" >/dev/null
in_ws toy-api security-review recap set \
  --goal "Find the worst bug in auth.py and get it fixed by a teammate" --goal-short "auth.py review, delegate fix" \
  --state "Reviewing src/auth.py" --state-short "reviewing auth.py" \
  --next "Delegate the fix over wsx agent send" --next-short "delegate fix" >/dev/null
CLAUDE_ID="$(in_ws toy-api security-review agent list | awk '$2=="claude"{print $1}')"
PEER_ID="$(in_ws toy-api security-review agent list | awk -v l="$PEER_LABEL" '$2==l{print $1}')"
echo "staged toy-api/security-review (claude=$CLAUDE_ID $PEER_LABEL=$PEER_ID)"

[ -n "${STAGE_ONLY:-}" ] && { echo "STAGE_ONLY set — skipping render"; exit 0; }

# shellcheck source=/dev/null
source "$HERE/still.sh"
trap still_down EXIT
still_up 166 36
still_keys z a; sleep 0.5; still_keys Down; still_keys Enter     # attach → Claude pane
sleep 12

# Claude reviews and delegates. Its `wsx agent send <peer> …` lands in the mail
# table; delivery spawns the peer and injects the banner.
wait_sql "claude → peer queued" 240 "select count(*) from agent_messages where target_agent_id=$PEER_ID and from_agent_id=$CLAUDE_ID"
sleep 3; still_shot "$OUT/agent-chat-1.png"                       # Claude: review + 'queued message to <peer>'
wait_sql "claude → peer delivered" 120 "select count(*) from agent_messages where target_agent_id=$PEER_ID and delivered_at is not null"

# The peer's side: the [message from claude] banner. The injected message can
# land unsubmitted at the peer's prompt, so submit it (a no-op if it already went).
still_keys C-x; still_keys w; sleep 4
still_shot "$OUT/agent-chat-2.png"                                # peer: hand-off banner
still_keys Enter
wait_sql "peer → claude queued" 300 "select count(*) from agent_messages where target_agent_id=$CLAUDE_ID and from_agent_id=$PEER_ID"
sleep 3; still_shot "$OUT/agent-chat-3.png"                       # peer: fixed, committed, reported back

# Back to Claude: it receives [message from <peer>] and verifies the commit.
still_keys C-x; still_keys q
wait_sql "peer → claude delivered" 120 "select count(*) from agent_messages where target_agent_id=$CLAUDE_ID and delivered_at is not null"
sleep 15; still_shot "$OUT/agent-chat-4.png"
sleep 20; still_shot "$OUT/agent-chat-5.png"
sleep 25; still_shot "$OUT/agent-chat-6.png"
echo "stills in demo/out/agent-chat-*.png"
