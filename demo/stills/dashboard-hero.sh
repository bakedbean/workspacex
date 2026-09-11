#!/usr/bin/env bash
# Stage a "dashboard under load" scene in the sandbox and screenshot it.
#
# Builds on an already-provisioned sandbox (sandbox/bootstrap.sh). Everything is
# seeded from the CLI before the TUI launches — workspaces are created with
# --prompt so the queued starter message makes the dashboard spawn every agent
# in the background on its first ticks, no per-row attach choreography needed:
#
#   - three repos (toy-api / toy-cli / toy-web), three workspaces each
#   - toy-api/security-review runs Claude AND Codex; toy-cli/close-file-handle
#     is Codex-primary — so the agent strip shows more than one harness
#   - most workspaces carry a goal / state / next recap, which renders inline
#     in the row and in the detail bar for the selected row
#   - one agent is told to ask a question (the `?` status), one finishes at
#     once (`✓`), the rest are mid-task (spinner)
#
# Needs tmux and Chrome/Chromium (CHROME_BIN to override the lookup).
#
# Usage: demo/stills/dashboard-hero.sh          # after sandbox/bootstrap.sh
#        WSX_BIN=./target/debug/wsx demo/stills/dashboard-hero.sh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/sandbox/env.sh"
WSX="${WSX_BIN:-wsx}"
WSX="$(command -v "$WSX")"
WSX_BIN_DIR="$(cd "$(dirname "$WSX")" && pwd)"

# shellcheck source=/dev/null
source "$HERE/scene.sh"

# Taller detail bar: at ~36 rows the default 30% leaves a gap under the list.
"$WSX" config set detail_bar_config '{"height": {"percent": 45, "max_rows": 18}}' >/dev/null

ws toy-api security-review claude \
  "Review src/auth.py and src/app.py for security bugs. Report each finding with file and line. Do NOT edit files — your Codex teammate owns the fix. $FIX_AND_COMMIT"
in_ws toy-api security-review agent add codex >/dev/null
in_ws toy-api security-review agent send codex \
  "Fix the SQL injection in src/auth.py login() with a parameterized query, run python3 -m py_compile src/auth.py, commit with a conventional message, then reply Done." >/dev/null || true
recap toy-api security-review \
  "Audit auth.py for injection and auth bypass, ship the fixes via Codex" "audit auth.py, fixes via codex" \
  "SQL injection confirmed in login(); Codex applying a parameterized query" "SQLi confirmed, codex fixing" \
  "Verify the query, then review is_admin() token check" "verify query, then is_admin()"
status toy-api security-review working "reviewing auth.py; codex has the SQLi fix"

scene_background_workspaces

echo "staged $("$WSX" workspace list | wc -l | tr -d ' ') workspaces"

# --- Shoot it: tmux drives the TUI, Chrome renders the colored capture ---
# STAGE_ONLY=1 stops here, leaving the seeded sandbox for a manual look
# (e.g. `harness/harness.sh capture` or a tmux session running the TUI).
[ -n "${STAGE_ONLY:-}" ] && { echo "STAGE_ONLY set — skipping render"; exit 0; }
mkdir -p "$ROOT/demo/out"
OUT="$ROOT/demo/out"
# shellcheck source=/dev/null
source "$HERE/still.sh"
trap still_down EXIT
still_up 166 36
# Expand every repo, then select the first workspace so the detail bar opens.
still_keys z a; sleep 0.5; still_keys Down
# Agents are booting from their queued starter prompts. Early takes catch the
# most spinners; later ones carry diffs, agent-written recaps and RECENT CHAT.
# Several takes, cycling the selected row, so the best moment can be picked.
sleep 30;  still_shot "$OUT/dashboard-hero-1.png"
sleep 15;  still_shot "$OUT/dashboard-hero-2.png"
still_keys Down; sleep 15;  still_shot "$OUT/dashboard-hero-3.png"
still_keys Down; sleep 20;  still_shot "$OUT/dashboard-hero-4.png"
still_keys Up; still_keys Up; sleep 20; still_shot "$OUT/dashboard-hero-5.png"
sleep 30;  still_shot "$OUT/dashboard-hero-6.png"
still_keys Down; still_keys Down; sleep 30; still_shot "$OUT/dashboard-hero-7.png"
still_keys Up; still_keys Up; sleep 40; still_shot "$OUT/dashboard-hero-8.png"
echo "stills in demo/out/dashboard-hero-*.png"
