# Clip 2 — Parallel worktrees: spin up isolated worktrees across repos, deploy a
# coding agent to each, and watch them work in parallel on the dashboard.
Output demo/out/02-parallel-raw.mp4
Set Shell "bash"
Set FontSize 14
Set Width 1280
Set Height 720
Set Padding 16
Set TypingSpeed 40ms
Set WaitTimeout 90s

Env XDG_STATE_HOME "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"
Env CODEX_HOME "/tmp/wsx-demo/codex-home"

# Spin up three isolated worktrees across two repos (real CLI).
Type "wsx workspace create toy-api --name add-rate-limit --yolo --agent claude"
Enter
Sleep 900ms
Type "wsx workspace create toy-api --name fix-auth --yolo --agent codex"
Enter
Sleep 900ms
Type "wsx workspace create toy-cli --name arg-parsing --yolo --agent claude"
Enter
Sleep 900ms

# Open the dashboard and expand all repos.
Type "wsx"
Enter
Sleep 2500ms
Type "za"
Sleep 1500ms
Screenshot demo/out/02-dashboard.png

# Deploy the same task to each agent, detaching after each — they keep running.
# add-rate-limit (claude)
Down
Enter
Wait+Screen /bypass permissions on/
Type "Review every source file for bugs and propose fixes with code."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 800ms

# fix-auth (codex)
Down
Enter
Wait+Screen /(OpenAI Codex|gpt-5)/
Type "Review every source file for bugs and propose fixes with code."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 800ms

# arg-parsing (claude) — Down past the toy-cli header.
Down
Down
Enter
Wait+Screen /bypass permissions on/
Type "Review every source file for bugs and propose fixes with code."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 800ms

# Back on the dashboard: three agents working in parallel. End on this held
# frame — do NOT quit (quitting would close the clip on the raw shell).
Sleep 2s
Screenshot demo/out/02-parallel-working.png
Sleep 6s
