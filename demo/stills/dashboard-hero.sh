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

# --- helpers ---
ws() { # <repo> <slug> <agent> <prompt>
  "$WSX" workspace create "$1" --name "$2" --yolo --agent "$3" --prompt "$4" >/dev/null
  echo "created $1/$2 ($3)"
}
in_ws() { # <repo> <slug> <cmd...>  — run a wsx command with cwd inside the worktree
  local path; path="$("$WSX" workspace path "$1" "$2")"
  (cd "$path" && "$WSX" "${@:3}")
}
recap() { # <repo> <slug> <goal> <goal-short> <state> <state-short> <next> <next-short>
  in_ws "$1" "$2" recap set --goal "$3" --goal-short "$4" \
    --state "$5" --state-short "$6" --next "$7" --next-short "$8" >/dev/null
}
status() { in_ws "$1" "$2" status set "$3" --message "$4" >/dev/null; }

# Taller detail bar: at ~36 rows the default 30% leaves a gap under the list.
"$WSX" config set detail_bar_config '{"height": {"percent": 45, "max_rows": 18}}' >/dev/null

FIX_AND_COMMIT="Work autonomously and ask no questions. Set your wsx recap and status as you go."

# --- toy-api ---
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

ws toy-api add-rate-limit claude \
  "Add src/ratelimit.py with a token-bucket RateLimiter and a sliding-window SlidingWindowLimiter, each with an injectable clock. Write a thorough unittest suite (at least 15 tests, including a threaded concurrency test), run it with python3 -m unittest, then wire the limiter into handle() in src/app.py and document it in README.md. Commit in three logical commits. $FIX_AND_COMMIT"
recap toy-api add-rate-limit \
  "Token-bucket rate limiter for /login" "token-bucket limiter, /login" \
  "ratelimit.py written, unit tests running" "tests running" \
  "Wire the limiter into handle() and commit" "wire into handle()"
status toy-api add-rate-limit working "writing token-bucket tests"

ws toy-api fix-auth claude \
  "Fix the auth bypass in is_admin() in src/auth.py: compare the token against an ADMIN_TOKENS set read from the environment using a constant-time comparison. Then also fix the SQL injection in login() with a parameterized query. Write unittests for both (at least 10 cases, including empty, missing and malformed tokens), run them, and commit each fix separately. Finally write SECURITY.md describing both issues and the fixes, and commit it. $FIX_AND_COMMIT"
recap toy-api fix-auth \
  "Close the is_admin() auth bypass with a real token check" "close is_admin() bypass" \
  "Constant-time compare against ADMIN_TOKENS in place" "hmac.compare_digest in" \
  "Add the unittest, then commit" "add unittest, commit"
status toy-api fix-auth working "adding is_admin unittest"

# --- toy-cli ---
ws toy-cli arg-parsing claude \
  "Fix the off-by-one in parse_args in src/main.py so the first real argument is not skipped, then add proper --help and --version handling with argparse and a friendly error when no config path is given. Write unittests covering no args, one arg, many args, --help and --version (at least 10 cases), run them, and commit in two commits. $FIX_AND_COMMIT"
recap toy-cli arg-parsing \
  "Fix parse_args dropping the first argument" "parse_args off-by-one" \
  "argv[1:] fix applied, test written" "fix applied, test written" \
  "Run the suite and commit" "run suite, commit"
status toy-cli arg-parsing working "running unittest"

ws toy-cli close-file-handle codex \
  "Fix the leaked file handle in read_config in src/main.py using a with-block, verify with python3 -m py_compile, and commit. Work autonomously and ask no questions."
recap toy-cli close-file-handle \
  "Stop read_config leaking its file handle" "read_config handle leak" \
  "with-block rewrite in progress" "with-block rewrite" \
  "py_compile, then commit" "py_compile, commit"
# Codex sessions have no session log for wsx to derive activity from, so push
# the state the agent is actually in.
status toy-cli close-file-handle working "rewriting read_config with a with-block"

ws toy-cli config-loader claude \
  "I want to add a --config flag to the CLI. Before writing any code, ask me exactly one question: should the config file be TOML or JSON? Then stop and wait for my answer."
recap toy-cli config-loader \
  "Add a --config flag that loads settings from a file" "add --config flag" \
  "Blocked on file format decision" "blocked: file format" \
  "User picks TOML or JSON" "user picks TOML/JSON"

# --- toy-web ---
ws toy-web form-validation claude \
  "Fix validateForm in src/app.js so it requires a non-empty local part, an @, and a domain with a dot, and returns a structured {ok, errors} result. Write a node --test suite with at least 12 cases (empty, whitespace, unicode, multiple @, missing TLD), run it, add a Validation section to README.md, and commit in two commits. $FIX_AND_COMMIT"
recap toy-web form-validation \
  "Reject empty local parts in validateForm" "validateForm empty local" \
  "Regex check written, node --test running" "node --test running" \
  "Commit and open a PR" "commit, open PR"
status toy-web form-validation working "running node --test"

ws toy-web persist-dark-mode claude \
  "Make applyTheme in src/app.js persist the theme to localStorage, restore it on load, and fall back to the OS preference via matchMedia when nothing is stored. Add a node --test suite using a small fake document/localStorage/matchMedia (at least 8 cases), run it, and commit in two commits. $FIX_AND_COMMIT"
recap toy-web persist-dark-mode \
  "Persist the dark-mode preference across reloads" "persist dark mode" \
  "localStorage write added; restore-on-load next" "storage write added" \
  "Restore on load, then commit" "restore on load, commit"

ws toy-web list-render-perf claude \
  "Reply with only the word Done and ask no questions."
recap toy-web list-render-perf \
  "Batch DOM writes in renderList with a DocumentFragment" "batch renderList DOM writes" \
  "DocumentFragment rewrite committed; 40x fewer reflows" "fragment rewrite committed" \
  "Open the PR" "open PR"
status toy-web list-render-perf done "renderList rewrite committed"

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
