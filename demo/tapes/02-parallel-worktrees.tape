# Clip 2 — Parallel worktrees with live detail bars.
# Create three isolated worktrees from the dashboard (Shift+N, permissive) and
# deploy an agent to FIX + commit the bugs in each — created and attached one at
# a time (cursor stays put, no fragile dashboard navigation). Then tour the
# detail bars (SESSION SUMMARY / RECENT CHAT / RECENT FILES with +X −Y line
# counts). Rendered via sandbox/render.sh (clears Claude Code markers).
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

Type "wsx"
Enter
Sleep 3s

# add-rate-limit: create from the dashboard, then deploy.
Type "N"
Sleep 1200ms
Type "add-rate-limit"
Sleep 600ms
Screenshot demo/out/02-modal.png
Enter
Sleep 2s
Enter
Wait+Screen /bypass permissions on/
Type "Fix the SQL injection in src/auth.py with a parameterized query and commit it. Then reply with only the word Done and ask no questions."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 1500ms

# fix-auth
Type "N"
Sleep 1200ms
Type "fix-auth"
Sleep 600ms
Enter
Sleep 2s
Enter
Wait+Screen /bypass permissions on/
Type "Fix the SQL injection in src/auth.py with a parameterized query and commit it. Then reply with only the word Done and ask no questions."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 1500ms

# null-guard
Type "N"
Sleep 1200ms
Type "null-guard"
Sleep 600ms
Enter
Sleep 2s
Enter
Wait+Screen /bypass permissions on/
Type "Fix the SQL injection in src/auth.py with a parameterized query and commit it. Then reply with only the word Done and ask no questions."
Enter
Sleep 1500ms
Ctrl+x
Type "d"
Sleep 1500ms
Screenshot demo/out/02-dashboard.png

# Let all three fix + commit; wsx tails the logs and re-polls the per-file diffs.
Sleep 72s

# Tour the populated detail bars — dashboard is now fully settled (3 complete).
Type "za"
Sleep 2500ms
Screenshot demo/out/02-dashboard-done.png
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 4s
Screenshot demo/out/02-detail-1.png
Down
Sleep 4s
Screenshot demo/out/02-detail-2.png
Down
Sleep 4s
Screenshot demo/out/02-detail-3.png
Sleep 2s
