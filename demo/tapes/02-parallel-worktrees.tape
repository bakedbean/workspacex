# Clip 2 — Parallel worktrees with live detail bars.
# Spin up isolated worktrees, deploy a reviewing agent to each, then tour the
# dashboard detail bars (SESSION SUMMARY / RECENT CHAT) as they fill with each
# agent's findings. Rendered via demo/render.sh (clears Claude Code markers so
# the nested agents persist their session logs).
Output demo/out/02-parallel-raw.mp4
Set Shell "bash"
Set FontSize 13
Set Width 1280
Set Height 720
Set Padding 14
Set TypingSpeed 35ms
Set WaitTimeout 120s

Env XDG_STATE_HOME "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"

# Spin up three isolated worktrees across two repos.
Type "wsx workspace create toy-api --name add-rate-limit --yolo --agent claude"
Enter
Sleep 800ms
Type "wsx workspace create toy-api --name fix-auth --yolo --agent claude"
Enter
Sleep 800ms
Type "wsx workspace create toy-cli --name arg-parsing --yolo --agent claude"
Enter
Sleep 800ms

# Dashboard, expand all repos.
Type "wsx"
Enter
Sleep 2500ms
Type "za"
Sleep 1500ms
Screenshot demo/out/02-dashboard.png

# Deploy a review task to each agent (attach -> prompt -> detach; keeps running).
Down
Enter
Wait+Screen /bypass permissions on/
Type "Review every source file in this repo and summarize the bugs you find."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 800ms

Down
Enter
Wait+Screen /bypass permissions on/
Type "Review every source file in this repo and summarize the bugs you find."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 800ms

Down
Down
Enter
Wait+Screen /bypass permissions on/
Type "Review every source file in this repo and summarize the bugs you find."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 800ms

# Let all three finish a turn; wsx tails the logs and writes each SESSION SUMMARY.
Sleep 45s

# Tour the populated detail bars: cursor is on arg-parsing; walk up through each.
Sleep 3500ms
Screenshot demo/out/02-detail-1.png
Up
Up
Sleep 3500ms
Screenshot demo/out/02-detail-2.png
Up
Sleep 4s
Screenshot demo/out/02-detail-3.png
Sleep 2s
