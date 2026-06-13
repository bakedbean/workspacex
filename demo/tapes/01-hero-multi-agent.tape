# Clip 1 — Hero: create a workspace in the TUI, two different agents collaborate
# on it (Claude reviews -> Codex fixes), then close on the dashboard showing the
# work landed (status + populated detail bar). Rendered via demo/render.sh.
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

# --- INTRO: spin up the workspace from the dashboard ---
Type "wsx"
Enter
Sleep 3s
Screenshot demo/out/01-intro-dashboard.png
# Shift+N opens the New Workspace modal in permissive mode.
Type "N"
Sleep 1500ms
Type "security-review"
Sleep 1s
Screenshot demo/out/01-intro-modal.png
Enter
Sleep 2s
# the new workspace is selected on the dashboard — attach to it.
Enter
Wait+Screen /bypass permissions on/
Sleep 1s

# --- Claude reviews and finds the planted bug ---
Type "Review src/auth.py and name the most serious security bug in one sentence."
Enter
Wait+Screen@140s /(SQL inject|injection)/
Sleep 3s
Screenshot demo/out/01-claude-found.png

# --- Add a second agent (Codex) to the SAME workspace and fix it ---
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
Ctrl+x
Type "w"
Sleep 5s
Type "Fix the SQL injection in src/auth.py with parameterized queries. Keep it brief."
Enter
Sleep 30s
Screenshot demo/out/01-codex-fixed.png

# --- OUTRO: detach to the dashboard; the work shows up (status + detail bar) ---
Ctrl+x
Type "d"
Sleep 9s
Screenshot demo/out/01-outro-dashboard.png
Sleep 3s
