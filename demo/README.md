# wsx demo-screencast harness

Reproducible tooling that records captioned screencasts and stills of wsx
driving **live** coding agents — without anyone hand-driving the TUI. It stands
up a fully isolated wsx install with synthetic repos, drives the real `wsx` TUI
in a detached tmux server, and renders what tmux captures through headless
Chrome, in the TUI's real colors.

> Provisioning is shared with the e2e test harness — see
> [`../sandbox/README.md`](../sandbox/README.md) (env contract) and
> [`../harness/README.md`](../harness/README.md) (running the app for tests). This `demo/`
> dir is the screencast/still production layer on top of `sandbox/`.

[`SPIKE-NOTES.md`](SPIKE-NOTES.md) records the hard-won agent mechanics (config
isolation, trust pre-seeding, agent-pane switching, agent-to-agent
coordination, detail-bar population). It was written against the original VHS
tapes, which tmux replaced; the agent-side findings still hold.

## Clips

| Clip | Shows | Output |
|---|---|---|
| **hero** (~70s) | One workspace, two agents coordinating with no human in the loop: the workspace is created from the dashboard's bottom drawer and Codex is added from the agents drawer; Claude reviews & finds the planted SQL-injection bug, then hands the fix to Codex over `wsx agent send`; Codex fixes, commits, and reports back the commit hash; Claude verifies. | `out/01-hero.mp4` |
| **parallel** (~70s) | Three isolated worktrees in one repo (`toy-api`), each created from the bottom drawer with an agent fixing + committing one planted bug, then a tour of the live detail bars (SESSION SUMMARY / RECENT CHAT / RECENT FILES with `+X −Y` counts). It ends by tagging one workspace from the `#` drawer and hiding that tag from the `F` drawer, which folds it into a `▸ #parked · 1 hidden` group row that expands in place. | `out/02-parallel.mp4` |

## Stills

| Still | Shows | Output |
|---|---|---|
| **dashboard-hero** | The dashboard under load: three repos, nine live workspaces, Claude + Codex sharing one of them, `?`/`✓`/spinner statuses, workspace tags on the rows, `#parked` workspaces folded into hidden-group rows, recaps inline and in the detail bar. Used as the site hero image. | `out/dashboard-hero-N.png` (several takes) |
| **dashboard-drawer** | The same scene with the `F` hide-rules picker open in the bottom drawer: tags with their counts above the PR states. Shot at the end of `hero-still`. | `out/dashboard-drawer-N.png` |
| **agent-chat** | The attached view: Claude reviews, hands the fix to Codex over `wsx agent send`, Codex fixes + commits + reports back, Claude verifies — with the pinned-command chips, the footer agents row, and the hero scene's other workspaces filling the top bar as they finish or ask. Progress is gated on wsx's own mail table, not sleeps. | `out/agent-chat-N.png` (several takes) |

```bash
make -C demo hero-still         # bootstrap + demo/stills/dashboard-hero.sh
make -C demo agent-chat-still   # bootstrap + demo/stills/agent-chat.sh
```

`demo/stills/dashboard-hero.sh` seeds the whole scene from the CLI — workspaces
are created with `--prompt`, so the dashboard spawns every agent in the
background on launch and no per-row attach choreography is needed — then drives
the TUI in tmux and renders the colored capture through headless Chrome
(`demo/stills/still.sh`).

Both stills dress the bars in the orange example bar theme
(`docs/examples/theme-orange.toml`) over the jellybeans palette, installed into
the sandbox's own `$XDG_CONFIG_HOME/wsx/theme.toml`. `STILL_THEME=<file>` shoots
another theme file (`STILL_PALETTE` picks the base palette it pairs with);
`STILL_THEME=` (empty) shoots the stock bars. The rendering font is
`FiraCode Nerd Font` when installed, so the powerline caps come out as glyphs.

## Prerequisites

- `tmux`, `ffmpeg` (with libx264), `node` ≥ 22 (built-in `WebSocket`), `python3`,
  `sqlite3`, and Chrome or Chromium (`CHROME_BIN` to override the lookup).
  FiraCode Nerd Font, when installed, renders the powerline caps and icons.
- The `claude` and `codex` CLIs installed and **logged in** (the harness copies
  their credentials into an isolated config — see Isolation below).
- Run with `WSX_BIN=./target/debug/wsx` (or another local build) to shoot the
  code in this checkout rather than whatever `wsx` is on PATH.

## Usage

```bash
make -C demo clips      # render both clips end-to-end
make -C demo hero       # just the hero clip
make -C demo parallel   # just the parallel clip
make -C demo check      # unit-check the timeline builder (no recording)
make -C demo clean      # wipe the sandbox and out/
```

Everything in `out/` is **gitignored** — the committed deliverable is the harness
that regenerates the clips. Each clip keeps its capture in `out/rec-<name>/`, so
`bash demo/clips/render.sh out/rec-<name> out/<name>.mp4` re-renders a take
(e.g. after a caption-style change) without re-running the agents.

## Pipeline

Each clip script (`demo/clips/0N-*.sh`) runs these stages:

1. **`sandbox/bootstrap.sh`** (from the Makefile) — fresh isolated wsx install +
   synthetic repos + pre-authed/pre-trusted Claude & Codex configs + the **wsx
   agent skill** copied into both isolated configs + session-log symlinks.
2. **`demo/stills/still.sh`** — starts the TUI in a detached tmux server with the
   Claude Code parent-session markers cleared (so agents persist their session
   logs and the detail bars populate), and sends keys.
3. **`demo/clips/record.sh`** — samples `tmux capture-pane -e` about eight times
   a second, keeping only changed frames, and timestamps marks the script
   issues: `rec_caption` (caption from now on), `rec_speed` (play what follows
   N× faster — used while agents work), `rec_wait` (block on a screen regex),
   `rec_type` (type like a person). Progress waits on wsx's own mail table or on
   commits, not sleeps.
4. **`demo/clips/timeline.py`** — gives each frame its on-screen time divided by
   the speed in force, capped at `MAX_HOLD` (2.5s) so boots and idle stretches
   never show as dead air, and renders each frame to HTML with
   `stills/ansi2html.py`.
5. **`demo/clips/shoot.mjs`** — one headless Chrome over the DevTools protocol
   screenshots every frame into `frame.html`: the terminal above a caption band,
   so captions never cover the bottom drawer.
6. **`demo/clips/render.sh`** — glues 4–5 together and encodes the PNGs at their
   durations to a 1920×1080 H.264 MP4 (`SCALE`, `CRF`, `MAX_HOLD` tune it).

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
- In the hero clip, Claude (after reviewing) runs `wsx agent send codex "<bug + location
  + fix>"`; wsx prints the deterministic `queued message #<id> to codex (<n> bytes)`
  (the script waits on the `agent_messages` row instead). Codex receives it as a
  `[message #<id> from claude; …]` banner, fixes + commits, then `wsx agent send claude` back with the hash.
- **Known quirk (worked around in the clip script):** a message from Claude often lands in
  Codex's prompt *unsubmitted*. After switching to Codex (`Ctrl-x w`) the script
  presses `Enter` to submit it. Once submitted, Codex reliably replies.

## Customizing

- **Repos / planted bugs:** `sandbox/gen-repos.sh`.
- **What each agent does / pacing / captions:** the clip scripts in
  `demo/clips/`. Captions are issued inline with `rec_caption`, so they stay on
  the beat they describe however long the agents take.
- **Dead-air cap / quality / resolution:** `MAX_HOLD`, `CRF`, `SCALE` for
  `demo/clips/render.sh`.
- **Tests:** `make -C demo check` runs `clips/test-timeline.sh` (no recording).
  The provisioning tests live in `sandbox/`: run them with
  `bash sandbox/test-{gen-repos,bootstrap,agent-env,env}.sh`.
