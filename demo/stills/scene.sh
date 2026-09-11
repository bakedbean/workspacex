#!/usr/bin/env bash
# Shared staging for the stills: CLI helpers plus the "dashboard under load"
# background scene (eight workspaces across toy-api / toy-cli / toy-web, each
# created with --prompt so the TUI spawns its agent on launch). `source` this
# after sandbox/env.sh with $WSX set to the wsx binary.
#
#   ws <repo> <slug> <agent> <prompt>          create a workspace with a starter prompt
#   in_ws <repo> <slug> <wsx args…>            run wsx with cwd inside that worktree
#   recap <repo> <slug> <goal> <goal-short> <state> <state-short> <next> <next-short>
#   status <repo> <slug> <state> <message>
#   scene_background_workspaces                 stage the eight background workspaces

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

# Tail for every "just do it" prompt; the stills' own workspaces reuse it.
FIX_AND_COMMIT="Work autonomously and ask no questions. Set your wsx recap and status as you go."

scene_background_workspaces() {

  # --- toy-api ---
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
}
