Output test/out/dashboard.gif
Set Width 1200
Set Height 700
Set Shell "bash"
Env XDG_STATE_HOME "/tmp/wsx-test/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-test/claude-config"
Env CODEX_HOME "/tmp/wsx-test/codex-home"
Type "target/debug/wsx"
Enter
Sleep 3s
Screenshot test/out/dashboard.png
Sleep 1s
