# Clip 2 — Parallel worktrees: spin up isolated worktrees across repos, deploy a
# coding agent to each, and watch them work in parallel on the dashboard.
Output demo/out/02-parallel-raw.mp4
Set Shell "bash"
Set FontSize 14
Set Width 1280
Set Height 720
Set Padding 16
Set TypingSpeed 42ms
Set WaitTimeout 60s

Env XDG_STATE_HOME "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"
Env CODEX_HOME "/tmp/wsx-demo/codex-home"

# Spin up three isolated worktrees across two repos (real CLI).
Type "wsx workspace create toy-api --name add-rate-limit --yolo --agent claude"
Enter
Sleep 1500ms
Type "wsx workspace create toy-api --name fix-auth --yolo --agent codex"
Enter
Sleep 1500ms
Type "wsx workspace create toy-cli --name arg-parsing --yolo --agent claude"
Enter
Sleep 1500ms

# Open the dashboard and expand all repos.
Type "wsx"
Enter
Sleep 3s
Type "za"
Sleep 2s
Screenshot demo/out/02-dashboard.png

# Deploy a task to add-rate-limit (claude), then detach — it keeps running.
Down
Enter
Sleep 7s
Type "Review every source file for bugs and propose fixes with code."
Enter
Sleep 3s
Ctrl+x
Type "d"
Sleep 2s

# fix-auth (codex).
Down
Enter
Sleep 9s
Type "Review every source file for bugs and propose fixes with code."
Enter
Sleep 3s
Ctrl+x
Type "d"
Sleep 2s

# arg-parsing (claude) — Down past the toy-cli header.
Down
Down
Enter
Sleep 7s
Type "Review every source file for bugs and propose fixes with code."
Enter
Sleep 3s
Ctrl+x
Type "d"
Sleep 2s

# Back on the dashboard: three agents working in parallel.
Sleep 5s
Screenshot demo/out/02-parallel-working.png
Sleep 2s
Type "q"
Sleep 1s
