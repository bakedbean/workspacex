# Spike (final): single live claude agent, isolated+pre-trusted config, clean boot.
Output demo/out/spike-live.mp4
Set Shell "bash"
Set FontSize 15
Set Width 1280
Set Height 720
Set Padding 16
Set TypingSpeed 55ms
Set WaitTimeout 175s

Env XDG_STATE_HOME "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"

# Create the review workspace (shown as a real CLI command).
Type "wsx workspace create toy-api --name security-review --yolo --agent claude"
Enter
Sleep 2s

# Launch the dashboard.
Type "wsx"
Enter
Sleep 4s
Screenshot demo/out/spike-live-dashboard.png

# Unfold the repo, select the workspace, attach (boots straight to a ready agent).
Type "l"
Sleep 1s
Down
Sleep 1s
Enter
Sleep 8s
Screenshot demo/out/spike-live-agent-ready.png

# Ask for the review; block on the real agent finding the planted bug.
Type "In one sentence, name the single most serious security bug in src/auth.py."
Enter
Wait+Screen@170s /(SQL|inject|njection)/
Sleep 3s
Screenshot demo/out/spike-live-answer.png

Ctrl+x
Type "d"
Sleep 2s
Type "q"
Sleep 1s
