Output demo/out/mini.mp4
Set Shell "bash"
Set FontSize 14
Set Width 1280
Set Height 720
Set Padding 16
Set TypingSpeed 45ms

Env XDG_STATE_HOME "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"
Env CODEX_HOME "/tmp/wsx-demo/codex-home"

Type "wsx"
Enter
Sleep 4s
Type "l"
Sleep 800ms
Down
Sleep 800ms
Enter
Sleep 8s
Screenshot demo/out/mini-1-attached.png
Ctrl+x
Type "a"
Sleep 2s
Screenshot demo/out/mini-2-panel.png
Down
Down
Down
Sleep 800ms
Screenshot demo/out/mini-3-nav.png
Enter
Sleep 10s
Screenshot demo/out/mini-4-added.png
Ctrl+x
Type "w"
Sleep 5s
Screenshot demo/out/mini-5-switch.png
