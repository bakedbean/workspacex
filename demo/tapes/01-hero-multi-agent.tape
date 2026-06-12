# Clip 1 — Hero: two different coding agents on ONE workspace.
# Claude reviews and finds the bug; Codex is added to the same worktree and fixes it.
Output demo/out/01-hero-raw.mp4
Set Shell "bash"
Set FontSize 14
Set Width 1280
Set Height 720
Set Padding 16
Set TypingSpeed 45ms
Set WaitTimeout 150s

Env XDG_STATE_HOME "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"
Env CODEX_HOME "/tmp/wsx-demo/codex-home"

# One workspace, Claude as the primary agent.
Type "wsx workspace create toy-api --name security-review --yolo --agent claude"
Enter
Sleep 1500ms

# Open the dashboard and attach.
Type "wsx"
Enter
Sleep 2500ms
Type "l"
Sleep 700ms
Down
Sleep 700ms
Enter
Wait+Screen /bypass permissions on/
Sleep 800ms

# Claude reviews and reports the planted bug (proven-reliable Wait anchor).
Type "Review src/auth.py and name the most serious security bug in one sentence."
Enter
Wait+Screen@140s /(SQL inject|injection)/
Sleep 3s
Screenshot demo/out/01-claude-found.png

# Add a second agent — Codex — to the SAME workspace (Ctrl-x a, pick codex).
# Fixed Sleeps from here (a missed Wait would discard the whole render).
Ctrl+x
Type "a"
Sleep 1500ms
Down
Down
Down
Sleep 700ms
Enter
Sleep 11s
Screenshot demo/out/01-codex-added.png

# Switch to the Codex pane (Ctrl-x leader + 'w'); it needed time to register.
Ctrl+x
Type "w"
Sleep 5s
Screenshot demo/out/01-codex-switched.png

# Codex implements the fix on the same worktree.
Type "Fix the SQL injection in src/auth.py with parameterized queries. Keep it brief."
Enter
Sleep 30s
Screenshot demo/out/01-codex-fixed.png

# Back to the Claude pane — both agents collaborated on one workspace.
Ctrl+x
Type "q"
Sleep 2500ms
Screenshot demo/out/01-back-to-claude.png

# Detach and quit.
Ctrl+x
Type "d"
Sleep 1500ms
Type "q"
Sleep 1s
