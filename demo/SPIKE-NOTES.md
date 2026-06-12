# Spike notes — driving wsx + live agents through VHS

Ground truth discovered while de-risking the screencast harness. Every tape is
written against this.

## Toolchain (installed to ~/.local/bin, no sudo)

- `vhs` 0.11.0 (charmbracelet) — declarative `.tape` → MP4. Supports `Wait+Screen /re/`.
- `ttyd` 1.7.7 — VHS dependency (web terminal).
- `agg` 1.9.0 — asciinema cast → GIF (fallback path only).
- `ffmpeg` (system) + `chromium` (system, `/usr/bin/chromium`) — VHS renders frames
  via a headless Chromium driving ttyd. Both confirmed present.

## VHS ⇄ wsx ⇄ agent: it works

- VHS renders the **wsx ratatui TUI cleanly** at 1280×720, FontSize 15 (content
  clips slightly at the right edge at size 15 — drop to 14 or widen if needed).
- **Nested PTYs render fine**: VHS → ttyd → wsx → claude. The attached agent TUI
  (Claude Code v2.1.175) draws correctly inside the wsx pane.
- A **live** claude agent reviewed the planted `src/auth.py` and reported the SQL
  injection in ~5s; `Wait+Screen /(SQL|inject|njection)/` matches its output.

## Driving method (TUI keystrokes)

Launch `wsx` (no args) → dashboard. Then:

| Intent | Keys |
|---|---|
| Unfold / fold focused repo | `l` / `h` |
| Move selection | `Down`/`Up` (or `j`/`k`) |
| Attach to workspace (spawns/resumes agent) | `Enter` on the workspace row |
| Detach pane back to dashboard | `Ctrl-x` then `d` |
| Quit (kills sessions) | `q` |
| Add agent to current workspace | `Ctrl-x` then `a` |

Type the review prompt directly into the attached agent pane, `Enter`, then
`Wait+Screen@<timeout> /regex/` to block on the real answer.

## The blocker, and the fix: isolated + pre-accepted Claude config

A fresh worktree triggers Claude Code's **"Quick safety check: Is this a project
you trust?"** dialog *before* it accepts any input. `--dangerously-skip-permissions`
does NOT bypass it, and Claude does not reliably persist the acceptance (killed
sessions lose it). Symptom in VHS: the typed prompt leaks onto the screen, the
`Wait` never matches, VHS reports `recording failed` and **discards all artifacts
including screenshots**.

Fix — a fully isolated, pre-authenticated, pre-accepted Claude config under
`CLAUDE_CONFIG_DIR` (built by `sandbox-bootstrap.sh`), so agents boot straight to
a ready prompt with zero dialogs and **zero changes to the real `~/.claude`**:

1. `export CLAUDE_CONFIG_DIR=$WSX_DEMO_ROOT/claude-config`
2. Copy `~/.claude/.credentials.json` → auth (Linux stores the OAuth token here;
   it relocates with `CLAUDE_CONFIG_DIR`).
3. Copy `~/.claude.json` → app-state, so no first-run onboarding/theme prompts.
4. Write `settings.json` with `{"skipDangerousModePermissionPrompt": true}` →
   suppresses the bypass-mode warning.
5. **Pre-seed trust**: in the copied `.claude.json`, set
   `projects["<worktree-abs-path>"].hasTrustDialogAccepted = true` for every
   deterministic worktree path (`$XDG_STATE_HOME/wsx/worktrees/<repo>/<slug>`).
   Verified: Claude reads this from `$CLAUDE_CONFIG_DIR/.claude.json` and skips
   the dialog.

Verified isolation: accepting trust in the sandbox did **not** write the demo
worktree path into the real `~/.claude.json`.

## Env propagation

wsx inherits its full process env when spawning an agent (`src/pty/session.rs`
build_claude_command: `for (k,v) in std::env::vars() { cmd.env(k,v) }`). So
setting `CLAUDE_CONFIG_DIR` (and `XDG_STATE_HOME`) in the tape's `Env` block
reaches the spawned agent. Tapes MUST set both:

```
Env XDG_STATE_HOME   "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"
```

## Debugging tip

VHS hides everything on a failed run. To see what an agent actually shows, drive
wsx in a dedicated tmux server and `capture-pane -p` at intervals:
`tmux -L dbg new-session -d -s s -x 150 -y 42 ; tmux -L dbg send-keys ...`.

## Harnesses solved (demo uses Claude + Codex; Pi dropped by request)

Same isolate-config + pre-accept-trust pattern works for each (all in
`sandbox-bootstrap.sh`), authenticated from copied creds, zero changes to real
config:

| Harness | Config env | Auth source | Trust handling |
|---|---|---|---|
| Claude | `CLAUDE_CONFIG_DIR` | `~/.claude/.credentials.json` | pre-seed `projects[path].hasTrustDialogAccepted=true` in `.claude.json` (keyed by **worktree** path) |
| Codex | `CODEX_HOME` | `~/.codex/auth.json` | pre-seed `[projects."<repo-root>"] trust_level="trusted"` in `config.toml` (keyed by **repo root**) |

Tapes MUST `Env` both dir vars plus `XDG_STATE_HOME` so wsx passes them to the
spawned agents. (Pi also works — `PI_CODING_AGENT_DIR` + offline `deepseek-v4-flash`,
non-blocking trust — but is left out of the demo content by request.)

## Still open (for the multi-agent hero clip)

The `Ctrl-x a` agents-panel choreography (add codex+pi to one workspace, pane
layout, typing into each pane, cross-agent `wsx agent send`) still needs a
dedicated spike before the hero tape is written.
