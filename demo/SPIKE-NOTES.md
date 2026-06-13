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

## Multi-agent choreography (for the hero clip)

Adding a second agent to a workspace and switching between them:

- **Add an agent:** while attached, `Ctrl-x` then `a` opens the agents panel.
  The "Add:" row lists `claude  pi  hermes  codex`; navigate it with **Down/Up**
  (NOT Left/Right — Right is a no-op), so `Down ×3` reaches `codex`. `Enter` adds
  it and closes the panel; the new agent spawns and boots on the same worktree.
- **Agents bar:** once ≥2 agents exist, the attached view grows a footer row
  `agents:  ▌claude q   ▎codex w` — `▌` marks the focused pane; `q`/`w` are the
  switch keys (pool: `q w r y i o p s h j`, primary first).
- **Switch panes:** **`Ctrl-x` + the switch key** — `Ctrl-x q` → claude,
  `Ctrl-x w` → codex. **The README is WRONG**: it says the bare key (no leader)
  switches, but bare `w` is typed into the focused agent as literal text; the
  `Ctrl-x` leader is required. `Ctrl-x ←/→` does NOT cycle agents either.
- **Typing:** after switching, normal typing goes to the now-focused pane.

**Timing gotcha (cost a failed hero render):** after `Enter` adds codex, it needs
**~10-11s** to spawn and register as a switch target. `Ctrl-x w` fired ~2s after
the add is a **no-op** (stays on claude). So: `Sleep 11s` after adding before
switching.

**VHS failure mode:** a `Wait` that never matches makes VHS print `recording
failed` and **discard the whole mp4 + all screenshots**. So `Wait` is only safe
on *proven* anchors. The hero uses `Wait` for the claude steps (`/bypass
permissions on/` = ready, `/(SQL inject|injection)/` = review done — both
reliable) and **fixed `Sleep`s for the entire codex add→switch→fix sequence**
(codex's edit has no reliable completion anchor; a missed regex there would throw
away a 90s render). Debug VHS failures by mirroring the tape in tmux with
`capture-pane`, or by re-running the suspect span as a tiny Sleep+Screenshot tape
(Screenshots survive; they're only discarded when a `Wait` times out).

## Workspace detail bars (SESSION SUMMARY / RECENT CHAT) — the big one

These populate from claude's session jsonl at
`~/.claude/projects/<encode(canonicalize(worktree))>/*.jsonl` (wsx uses
`dirs::home_dir()`, i.e. real `$HOME`, with NO env override —
`sessionx/src/extract.rs:599,765`). Getting them to populate in the sandbox took
a long hunt; the root cause was non-obvious:

1. **Nested Claude Code sessions don't persist.** When the harness runs *inside*
   a Claude Code session (e.g. an agent driving it), every child inherits
   `CLAUDECODE` / `CLAUDE_CODE_SESSION_ID` / `CLAUDE_CODE_CHILD_SESSION` /
   `CLAUDE_CODE_ENTRYPOINT` / `CLAUDE_CODE_EXECPATH` / `AI_AGENT` / `CLAUDE_EFFORT`.
   A claude spawned with those set treats itself as a NESTED CHILD and writes only
   a repo-keyed `…/<repo>/memory/` dir — never a per-worktree session jsonl. So
   wsx finds nothing → permanent "loading…". `demo/render.sh` clears these before
   launching VHS (no-op outside Claude Code). This was THE blocker — verified
   across isolated/real config, /tmp vs $HOME, synthetic vs real-history repos,
   1 vs N turns: nothing persisted until the markers were cleared.
2. **Base-dir mismatch.** With markers cleared, claude (under the isolated
   `CLAUDE_CONFIG_DIR`) writes the jsonl to `$CLAUDE_CONFIG_DIR/projects/<enc>` —
   but wsx reads `$HOME/.claude/projects/<enc>`. `sandbox-bootstrap.sh` symlinks
   each demo worktree's log dir into `~/.claude/projects` to bridge them. This is
   the ONLY thing written outside the sandbox (transient symlinks; `make clean`
   removes them); real `~/.claude.json` / `settings.json` stay untouched.
3. **Needs >=1 completed turn**, after which wsx tails the jsonl and (a few
   seconds later) generates the SESSION SUMMARY. The detail-bar clip must let
   agents finish a turn and pause ~10s before touring the bars.

Encoding: every non-alphanumeric char -> `-`
(`/tmp/wsx-demo/.../add-rate-limit` -> `-tmp-wsx-demo-...-add-rate-limit`).
