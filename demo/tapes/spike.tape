# Spike A: prove wsx TUI renders + accepts keys through VHS (no live agent yet).
Output demo/out/spike.mp4
Set Shell "bash"
Set FontSize 15
Set Width 1280
Set Height 720
Set Padding 16
Set TypingSpeed 60ms

Env XDG_STATE_HOME "/tmp/wsx-demo/state"

Type "wsx"
Enter
Sleep 4s
Screenshot demo/out/spike-dashboard.png
# try navigating the dashboard a touch
Down
Sleep 1s
Up
Sleep 1s
Screenshot demo/out/spike-after-nav.png
# quit
Type "q"
Sleep 1s
Ctrl+C
Sleep 500ms
