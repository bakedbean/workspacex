# wsx demo-screencast harness

Reproducible tooling that records polished, captioned screencasts of wsx driving
**live** coding agents — without anyone hand-driving the TUI. It stands up a
fully isolated wsx install with synthetic repos, drives the real `wsx` TUI with
[VHS](https://github.com/charmbracelet/vhs), collapses dead air, and burns in
captions under GitHub's 10MB asset cap.

See [`SPIKE-NOTES.md`](SPIKE-NOTES.md) for the hard-won mechanics (config
isolation, trust pre-seeding, agent-pane switching, VHS gotchas).

## Clips

| Clip | Shows | Output |
|---|---|---|
| **hero** | One workspace, two agents coordinating with no human in the loop: Claude reviews & finds a planted SQL-injection bug, then hands the fix to its Codex teammate over `wsx agent send`; Codex fixes, commits, and reports back the commit hash; Claude verifies. | `out/01-hero.mp4` |
| **parallel** | Three isolated worktrees across two repos, a reviewing agent deployed to each, then a tour of each workspace's live SESSION SUMMARY / RECENT CHAT detail bar as it fills with that agent's findings. | `out/02-parallel.mp4` |

## Prerequisites

- `vhs`, `ttyd`, `ffmpeg`, and a headless-capable `chromium` (VHS renders frames
  through it). `agg` is optional (GIF fallback).
- The `claude` and `codex` CLIs installed and **logged in** (the harness copies
  their credentials into an isolated config — see Isolation below).
- `python3` (used by `sandbox-bootstrap.sh` and `deadair.sh`).

`vhs`/`ttyd`/`agg` ship as static binaries — no root needed:

```bash
curl -fsSL https://github.com/charmbracelet/vhs/releases/latest/download/vhs_Linux_x86_64.tar.gz | tar xz -C ~/.local/bin --strip-components=1 vhs
curl -fsSL -o ~/.local/bin/ttyd https://github.com/tsl0922/ttyd/releases/latest/download/ttyd.x86_64 && chmod +x ~/.local/bin/ttyd
```

## Usage

```bash
make -C demo clips      # render both clips end-to-end
make -C demo hero       # just the hero clip
make -C demo parallel   # just the parallel clip
make -C demo check      # unit-check the scripted pieces (no recording)
make -C demo clean      # wipe the sandbox and out/
```

Finals land in `out/` (gitignored): `*-raw.mp4` (uncaptioned, for editing),
`*-collapsed.mp4` (dead air removed), and `NN-name.mp4` (final, captioned,
≤10MB).

## Pipeline

Each clip flows through these stages (chained by the `Makefile`):

1. **`sandbox-bootstrap.sh`** — fresh isolated wsx install + synthetic repos +
   pre-authed/pre-trusted Claude & Codex configs + session-log symlinks (so the
   workspace detail bars can find the agents' logs — see below).
2. **`render.sh tapes/*.tape`** — VHS drives the real `wsx` TUI with live agents.
   `render.sh` first clears the `CLAUDECODE` / `CLAUDE_CODE_*` parent-session env
   markers, so agents spawned while the harness runs *inside* a Claude Code
   session still run as genuine top-level sessions and persist their per-worktree
   session logs (required for SESSION SUMMARY / RECENT CHAT to populate). No-op
   for a normal terminal user.
3. **`deadair.sh`** — `freezedetect`-driven collapse of static stretches (agent
   boots/idle) to a brief hold; active content stays at natural 1×.
4. **`speedramp.sh`** *(hero only)* — speeds up the one long *actively-changing*
   span deadair can't touch (Codex churning through its fix/commit), leaving every
   reading-critical beat at 1×. Span is tuned to the take (Makefile `HERO_RAMP_*`).
5. **`post.sh`** — burns in `drawtext` captions (from `captions/*.txt`, via
   `textfile=` so any text is safe) and enforces a ≤9MB budget, stepping down
   fps/scale until it fits.

## Isolation (nothing touches your real setup)

Every `wsx`/agent invocation is redirected into `$WSX_DEMO_ROOT` (default
`/tmp/wsx-demo`):

- `XDG_STATE_HOME` → isolated wsx `state.db`, worktrees, logs.
- `CLAUDE_CONFIG_DIR` → copied credentials + `settings.json`
  (`skipDangerousModePermissionPrompt`) + pre-seeded per-worktree trust.
- `CODEX_HOME` → copied `auth.json` + `config.toml` with pre-seeded per-repo-root
  trust.

Your real `~/.local/state/wsx`, `~/.claude.json`, `~/.claude/settings.json`, and
`~/.codex` are never written. The **only** thing written outside the sandbox is a
set of transient symlinks under `~/.claude/projects/<encoded-worktree>` pointing
into the sandbox — these bridge the isolated session logs to where wsx reads them
(`dirs::home_dir()`, no env override) so the detail bars populate. `make clean`
removes them along with the whole sandbox.

## Customizing

- **Repos / planted bugs:** `gen-repos.sh`.
- **What each agent does / pacing:** the `.tape` files (see `SPIKE-NOTES.md` for
  the driving conventions and timing gotchas).
- **Captions:** `captions/*.txt` — tab-separated `start  end  text` (seconds),
  timed to the *collapsed* clip.
- **Dead-air aggressiveness:** `MIN_FREEZE` / `MAX_HOLD` in the `Makefile`.
