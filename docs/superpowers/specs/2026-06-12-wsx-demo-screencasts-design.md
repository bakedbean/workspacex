# wsx demo-screencast harness — design

**Date:** 2026-06-12
**Status:** approved (pending spec review)

## Goal

Produce professional, easy-to-follow screencasts of wsx that showcase its two
headline strengths — **running multiple worktrees in parallel** and **deploying
multiple coding agents to a workspace to perform code reviews** — without the
author having to hand-drive the TUI (the current process yields footage that is
too fast and reads as amateurish).

The deliverable is a **reproducible demo harness**: a set of committed scripts
that stand up a throwaway wsx install, generate synthetic repos, drive a live
multi-agent session, and render polished landscape clips end-to-end. Re-runnable
with one command so pacing can be iterated on without manual driving.

## Locked decisions

| Axis | Choice | Rationale |
|---|---|---|
| Agents | **Live** — real `claude` + `codex` + `pi`, recorded for real | Authenticity over determinism (author's call) |
| Output format | Short (≈15–40s) **silent, landscape, captioned** clips | Matches README's existing embedded MP4s; no voiceover |
| Venue | **README / GitHub** + **docs / landing page** | One feature per clip + a hero clip |
| Capture tool | **VHS** (`charmbracelet/vhs`) `.tape` scripts, headless render at 1280×720 | Declarative pacing; `Wait` blocks on live-agent completion |
| Captioning | **ffmpeg `drawtext`** lower-third overlays, scripted | Reproducible/hands-off; raw uncaptioned clips also delivered for editor polish |
| Sandbox | Isolated via `XDG_STATE_HOME=<tmpdir>` | wsx resolves `state.db`, `worktrees/`, `logs/` under it — cannot touch real state |
| Repos | **Synthetic** small git repos with planted issues | Fast, satisfying live reviews; no licensing/privacy concerns; reproducible |
| Hero scenario | **Multi-agent, one workspace** — Claude+Codex+Pi review together + cross-agent `wsx agent send` | Mirrors existing README hero; showcases multi-harness collaboration |
| Harness location | Committed under **`demo/`** in the wsx repo | Reusable project tooling — anyone can regenerate the README clips |

## Hard constraint: 10MB per asset

GitHub caps dropped-in asset uploads at **10MB**, and README embedding is the
primary use case. This is the dominant constraint on the output pipeline:

- **MP4-first.** Terminal/TUI content is mostly static text and compresses
  extremely well under H.264; a 30–40s 720p clip lands comfortably under 10MB at
  a sane CRF. GitHub renders dropped-in MP4s inline. MP4 is the primary README
  deliverable.
- **GIF only where it fits.** A 40s 720p GIF easily exceeds 10MB. GIFs are
  reserved for short loops with reduced fps/size/palette; where a clip cannot fit
  as a GIF it ships MP4-only.
- **`post.sh` enforces a hard budget** (target **≤9MB** for headroom): CRF +
  `maxrate` cap, then a final size check that **errors loudly and auto-steps down**
  fps/scale rather than silently shipping an oversized file.
- Bias toward **shorter clips** — the hero is tightened toward ≈30–35s.

## Components

Each is independently runnable; the `Makefile` chains them.

1. **`demo/sandbox-bootstrap.sh`** — exports `XDG_STATE_HOME=<tmpdir>`, creates a
   fresh `state.db`, registers the `claude`/`codex`/`pi` agents and the synthetic
   repos. Idempotent (safe to re-run; `make clean` wipes the sandbox).
2. **`demo/gen-repos.sh`** — generates the synthetic repos (a toy web API and a
   small CLI tool) with real commits, a base branch, and a handful of **narrow,
   deliberately planted** bugs/smells so live reviews are fast and land clean,
   on-screen findings.
3. **`demo/tapes/*.tape`** — one VHS tape per clip. Uses `Hide`/`Show`, `Type`,
   `Sleep`, and `Wait` (regex on screen content) to drive `wsx` with live agents,
   blocking on real completion rather than guessing with fixed sleeps.
4. **`demo/post.sh`** — ffmpeg pass: trim/speed up dead air, add timed
   `drawtext` lower-third captions, emit final MP4 (+ GIF where it fits), and
   enforce the 10MB budget gate.
5. **`demo/Makefile`** — `make clips` runs bootstrap → gen-repos → render tapes →
   post; `make clean` wipes the sandbox; per-clip targets for fast iteration.

Outputs land in `demo/out/` (gitignored): `*-raw.mp4` (uncaptioned, for editor
polish) and `*.mp4` / `*.gif` (final, captioned, budget-enforced).

## Clip lineup

- **Clip 1 — Hero (≈30–35s):** one workspace; deploy Claude+Codex+Pi; each
  reviews the planted issues; `wsx agent send` relays a finding between agents;
  dashboard shows all three live. Covers *multiple agents reviewing*.
- **Clip 2 — Parallel worktrees (≈30s):** spin up several workspaces across the
  synthetic repos; dashboard overview with live status chips. Covers *parallel
  worktrees*.
- **Clip 3 (optional / stretch):** diff review (custom diff command / lazygit) or
  the project-manager pane.

## De-risking

The **first implementation step is a smoke spike**: prove VHS can drive `wsx`
with a *real agent nested inside* (nested PTYs through VHS's tmux+ttyd) and that
`Wait`-on-screen-text fires correctly. If nesting misbehaves, fall back to
asciinema capture for the affected clips without re-litigating the overall plan.

## Key technical notes / risks

- **Isolation:** all `wsx` invocations run with `XDG_STATE_HOME` pointed at the
  sandbox tmpdir. Verified: wsx resolves `state.db`, `worktrees/`, `logs/` under
  `$XDG_STATE_HOME/wsx/`. No `--db` flag exists; the env override is the seam.
- **Agent binaries:** `claude`, `codex`, `pi` confirmed on PATH (`hermes` absent).
  Agents spawn via PTY; `WSX_<AGENT>_BIN` can override the binary if needed.
- **Hands-off recording:** agents spawn with `--yolo`
  (`--dangerously-skip-permissions`) so no permission prompts stall the tape.
- **Live cost/time:** real agents call real models — costs tokens and real
  wall-clock. Narrow, pointed review prompts on small repos keep each take fast.
- **Determinism:** `Wait`-on-screen-text decouples the tape from agent timing;
  remaining variance (review wording/length) is absorbed by the post trim/speed
  pass. Re-running may need 1–2 takes to get a clean one.

## Out of scope (YAGNI)

- Voiceover / narration.
- Motion-graphics callouts beyond ffmpeg lower-thirds (raw clips are delivered so
  the author can add these in a real editor if desired).
- Stubbed/fake agents (explicitly rejected in favor of live agents).
- A long-form narrated tutorial.
