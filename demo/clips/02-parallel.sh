#!/usr/bin/env bash
# Clip 2 — parallel agents across isolated worktrees, then keeping the list tidy.
# Three workspaces in toy-api are created from the dashboard's bottom drawer, an
# agent is deployed to fix + commit one planted bug in each, and the detail bars
# fill in (SESSION SUMMARY / RECENT CHAT / RECENT FILES with +X −Y counts). Then
# one workspace is tagged from the `#` drawer and hidden by that tag from the `F`
# drawer, folding into a `▸ #parked · 1 hidden` group row that expands in place.
#
# Usage: demo/clips/02-parallel.sh        # after sandbox/bootstrap.sh
#        WSX_BIN=./target/debug/wsx demo/clips/02-parallel.sh
# Output: demo/out/02-parallel.mp4 (capture kept in demo/out/rec-02-parallel)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/sandbox/env.sh"
WSX="$(command -v "${WSX_BIN:-wsx}")"
WSX="$(cd "$(dirname "$WSX")" && pwd)/$(basename "$WSX")"
OUT="$ROOT/demo/out"; mkdir -p "$OUT"
# shellcheck source=/dev/null
source "$ROOT/demo/stills/scene.sh"
# shellcheck source=/dev/null
source "$ROOT/demo/stills/still.sh"
# shellcheck source=/dev/null
source "$HERE/record.sh"

"$WSX" config set remote_control off >/dev/null
# Room for the tag after the branch name instead of a `+1` overflow count.
"$WSX" config set dashboard_branch_width 34 >/dev/null
READY='(bypass permissions on|auto mode on)'
TAIL="Commit it with a conventional message, then reply with only the word Done. Ask no questions."

deploy() { # <slug> <prompt>
  still_keys N; sleep 1.1
  rec_type "$1"; sleep 0.6
  still_keys Enter; sleep 2
  still_keys Enter
  rec_speed 3; rec_wait "$READY" 120; sleep 0.6
  rec_speed 4; rec_type "$2"; rec_speed 1; sleep 0.5
  still_keys Enter; sleep 1.5
  still_keys C-x; sleep 1; still_keys d; sleep 1.5
}

# Every workspace has its fix committed (or the timeout passes).
wait_done() { # <timeout-s> <slug...>
  local t=0 s n
  while :; do
    n=0
    for s in "${@:2}"; do
      [ -n "$(git -C "$("$WSX" workspace path toy-api "$s")" log --oneline main..HEAD 2>/dev/null)" ] && n=$((n + 1))
    done
    [ "$n" -eq $(( $# - 1 )) ] && return 0
    [ "$t" -ge "$1" ] && { echo "timeout: agents not done" >&2; return 0; }
    sleep 3; t=$((t + 3))
  done
}

# Move the dashboard selection to <slug>: the detail bar's header names the
# selected workspace, so step until it does.
select_ws() { # <slug>
  local i key=Up
  for ((i = 0; i < 18; i++)); do
    tmux -L "$STILL_SOCK" capture-pane -p | grep -Eq "⎇ demo/$1( |$)" && return 0
    [ "$i" -eq 6 ] && key=Down
    still_keys "$key"; sleep 0.4
  done
  echo "select_ws: $1 not reached" >&2; return 1
}

trap still_down EXIT
WSX_BIN="$WSX" still_up 156 40
rec_start "$OUT/rec-02-parallel"
sleep 1.5

rec_caption "Create isolated worktrees from the dashboard's bottom drawer"
deploy add-rate-limit "Fix the SQL injection in login() in src/auth.py with a parameterized query. $TAIL"
rec_caption "Deploy an agent to each one — every agent works in its own worktree"
deploy fix-auth "Fix the auth bypass in is_admin() in src/auth.py: only accept tokens listed in an ADMIN_TOKENS environment variable, compared in constant time. $TAIL"
deploy null-guard "Fix handle() in src/app.py so a failed login (login() returning None) returns {\"error\": \"invalid credentials\"} instead of crashing. $TAIL"

rec_caption "Three agents fixing three bugs in parallel"
rec_speed 6; wait_done 240 add-rate-limit fix-auth null-guard; sleep 6; rec_speed 1

rec_caption "Each workspace's edits and +/− line counts — live in the detail bar"
select_ws null-guard; sleep 3.5
select_ws fix-auth; sleep 3.5
select_ws add-rate-limit; sleep 3.5

rec_caption "Tag a workspace from the # drawer"
still_keys '#'; sleep 1.5
rec_type parked; sleep 0.8
still_keys Enter; sleep 1.2
still_keys Escape; sleep 2

rec_caption "Hide a tag from the F drawer — its workspaces fold into one group row"
still_keys F; sleep 2
still_keys Space; sleep 1.5
still_keys Escape; sleep 3

rec_caption "Expand the group in place whenever you need it"
still_keys Enter; sleep 2
still_keys Down; sleep 3
still_keys h; sleep 2.5
rec_caption ""
sleep 1
rec_stop
still_down; trap - EXIT

bash "$HERE/render.sh" "$OUT/rec-02-parallel" "$OUT/02-parallel.mp4"
