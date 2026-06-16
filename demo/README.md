# wsx demo-screencast harness

Reproducible tooling that records polished, captioned screencasts of wsx driving
**live** coding agents — without anyone hand-driving the TUI. It stands up a
fully isolated wsx install with synthetic repos, drives the real `wsx` TUI with
[VHS](https://github.com/charmbracelet/vhs), collapses dead air, and burns in
captions under GitHub's 10MB asset cap.

> Provisioning is shared with the e2e test harness — see
> [`../sandbox/README.md`](../sandbox/README.md) (env contract) and
> [`../test/README.md`](../test/README.md) (running the app for tests). This `demo/`
> dir is now just the screencast/video-production layer on top of `sandbox/`.

See [`SPIKE-NOTES.md`](SPIKE-NOTES.md) for the hard-won mechanics (config
isolation, trust pre-seeding, agent-pane switching, agent-to-agent coordination,
detail-bar population, VHS gotchas). **If you are an agent picking this up, read
`SPIKE-NOTES.md` end to end first** — every non-obvious failure mode and its fix
is recorded there.

## Clips

| Clip | Shows | Output |
|---|---|---|
| **hero** (~66s) | One workspace, two agents coordinating with no human in the loop: Claude reviews & finds a planted SQL-injection bug, then hands the fix to its Codex teammate over `wsx agent send`; Codex fixes, commits, and reports back the commit hash; Claude verifies. | `out/01-hero.mp4` |
| **parallel** (~61s) | Three isolated worktrees in one repo (`toy-api`), an agent deployed to each to fix + commit the planted bug, then a tour of each workspace's live detail bar (SESSION SUMMARY / RECENT CHAT / RECENT FILES with `+X −Y` line counts) as it fills in. | `out/02-parallel.mp4` |

## Prerequisites

- `vhs`, `ttyd`, `ffmpeg`, and a headless-capable `chromium` (VHS renders frames
  through it). `agg` is optional (GIF fallback).
- The `claude` and `codex` CLIs installed and **logged in** (the harness copies
  their credentials into an isolated config — see Isolation below).
- `python3` (used by `sandbox/bootstrap.sh` and `deadair.sh`).

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

Everything in `out/` is **gitignored** — the committed deliverable is the harness
that regenerates the clips, not the `.mp4`s. Intermediates: `*-raw.mp4`
(uncaptioned, straight from VHS), `*-collapsed.mp4` (dead air removed),
`*-fast.mp4` (hero only, after the speed-ramp); the final captioned ≤10MB clip is
`NN-name.mp4` (`01-hero.mp4`, `02-parallel.mp4`).

## Pipeline

Each clip flows through these stages (chained by the `Makefile`):

1. **`sandbox/bootstrap.sh`** — fresh isolated wsx install + synthetic repos +
   pre-authed/pre-trusted Claude & Codex configs + the **wsx agent skill** copied
   into both isolated configs (so agents know the `wsx agent send` CLI — see
   *Agent coordination* below) + session-log symlinks (so the workspace detail
   bars can find the agents' logs — see *Isolation* below).
2. **`sandbox/render.sh tapes/*.tape`** — VHS drives the real `wsx` TUI with live
   agents. `render.sh` first clears the `CLAUDECODE` / `CLAUDE_CODE_*` parent-session env
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

Every `wsx`/agent invocation is redirected into `$WSX_SANDBOX_ROOT` (default
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

## Agent coordination (the hero's whole point)

The hero shows agents working *together* over wsx's own CLI, no human relaying:

- wsx ships an **agent skill** (`../skills/wsx/SKILL.md`) that teaches agents the
  `wsx agent send <label> <message>` command. The real installer
  (`wsx setup install-skill`) writes to `~/.claude/skills` / `~/.codex/skills` via
  `dirs::home_dir()` and **ignores `CLAUDE_CONFIG_DIR` / `CODEX_HOME`**, so it
  can't target the sandbox — `sandbox/bootstrap.sh` copies the same skill into the
  isolated configs directly instead.
- In the tape, Claude (after reviewing) runs `wsx agent send codex "<bug + location
  + fix>"`; wsx prints the deterministic `queued message to codex` (the only safe
  `Wait` anchor for this step). Codex receives it as a `[message from claude]`
  banner, fixes + commits, then `wsx agent send claude` back with the hash.
- **Known quirk (worked around in the tape):** a message from Claude often lands in
  Codex's prompt *unsubmitted*. After switching to Codex (`Ctrl-x w`) the tape
  presses `Enter` to submit it. Once submitted, Codex reliably replies.

## Customizing

- **Repos / planted bugs:** `sandbox/gen-repos.sh`.
- **What each agent does / pacing:** the `.tape` files (see `SPIKE-NOTES.md` for
  the driving conventions and timing gotchas).
- **Captions:** `captions/*.txt` — tab-separated `start  end  text` (seconds),
  timed to the clip that `post.sh` actually captions: the **post-ramp** clip for
  the hero (`01-hero-fast.mp4`), the **collapsed** clip for parallel (no ramp).
- **Dead-air aggressiveness:** `MIN_FREEZE` / `MAX_HOLD` in the `Makefile`.
- **Protect a subtle beat from dead-air collapse:** `HERO_ADD_PROTECT` in the
  `Makefile` — a `start:end` window (raw-clip seconds, floats) `deadair.sh` keeps
  at full 1×. Needed for the agent-picker selector stepping across `claude · pi ·
  hermes · codex`, whose motion is too subtle for `freezedetect` and would
  otherwise collapse to a flash. The window starts a beat earlier to also span the
  preceding `Ctrl-x` "actions" overlay dwell: that ~1.8s dwell is under
  `MIN_FREEZE` so it wouldn't be collapsed on its own, but folding it into the same
  window keeps the whole overlay→picker stretch at 1× as one contiguous beat.
  Take-specific — re-confirm if you re-record.
- **Hero speed-ramp window:** `HERO_RAMP_START` / `HERO_RAMP_END` /
  `HERO_RAMP_FACTOR` in the `Makefile` — absolute seconds in the *collapsed* clip,
  tuned to the recorded take (re-confirm if you re-record; see `SPIKE-NOTES.md`).
- **Tests:** `make -C demo check` runs the screencast scripts `test-{post,speedramp}.sh`
  (no recording). The provisioning tests moved to `sandbox/`: run them with
  `bash sandbox/test-{gen-repos,bootstrap,agent-env,env}.sh`.
