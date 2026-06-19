# Recreate wsx Demo Screencasts — Design

**Date:** 2026-06-16
**Branch:** `recreate-demo-screencasts`
**Status:** Approved

## Problem

Design changes have landed on `main` since the demo-screencast harness merged
(PR #175, 2026-06-13) that make both committed clips stale — they no longer
reflect the current TUI. The two clips are embedded in the top-level `README.md`
as GitHub user-attachment assets:

- `README.md:7` — **parallel** clip ("Parallel Agent Sessions")
- `README.md:11` — **hero** clip ("Multi Agent Sessions")

Both must be recreated to reflect the current design, and the new design's
showcase elements should be deliberately featured (not merely re-rendered).

## What changed (and which clip it hits)

**Attached / agent-chat view — hits the HERO clip:**
- Keybind footer removed → replaced by a centered **`Ctrl-x` "actions" overlay**
  (rows `d/u/a/e/t/v/g/k/x`, with ↑↓ to move a highlight + Enter to fire). The
  direct letter shortcuts (`a`, `w`, `q`, `d`) still dispatch as before — the
  overlay is purely additive. Source: `src/ui/attached/nav_menu.rs`,
  `src/app/input.rs` (`dispatch_leader_action`, leader-armed ↑↓/Enter).
- Workspace indicator moved to the **top** with a separator rule
  (`bottom_line` → `info_line`).
- Chip row now shows a **`^x` menu hint**.

**Dashboard — hits the PARALLEL clip:**
- Repo headers realigned: right-justified names, flush-right paths, rule-filled
  left-pad, empty-repo label dropped (PR #190).
- Detail panel: module scrollbars hidden, reserved column reclaimed — this is
  exactly the "detail-bar tour" the parallel clip lingers on.
- Updates panel omits empty repos.

## Approach

Chosen: **re-record + showcase the new UI** while keeping each clip's proven
backbone (hero: create ws → add Codex → delegate → Codex fixes/commits/reports →
Claude verifies → detach; parallel: 3 worktrees → deploy agent to each →
detail-bar tour). Insert short dwell beats so the redesigned elements register,
refresh captions to name them, and re-tune all take-specific timing values.

Rejected: minimal re-render (overlay flashes too fast to read at current
pacing); full re-storyboard (scope not requested; backbones still tell the right
story).

## Concrete changes

### Hero tape (`demo/tapes/01-hero-multi-agent.tape`)
- At the first `Ctrl+x` (currently followed immediately by `Type "a"`), insert
  `Sleep ~1800ms` so the centered **"actions"** overlay is readable before `a`
  fires. `a` ("agents") is a listed row, so the beat reads coherently.
- Keep the dwell **under `MIN_FREEZE` (5s)**: the overlay is a static screen, so
  a ~2s hold renders without `deadair` collapsing it and needs no
  `HERO_ADD_PROTECT` entry. (New gotcha to record in `SPIKE-NOTES.md`.)
- Leave the `w`/`q` pane-switches quick — they're pane-cycle keys, not overlay
  rows; dwelling there would show a menu that doesn't list them.
- The top workspace indicator + separator rule + `^x` chip hint appear in all
  attached footage automatically — no keystroke change, captioned instead.
- Anchors safe: `Wait+Screen /bypass permissions on/` (Claude banner) and
  `Wait+Screen@120s /queued message to codex/` (wsx CLI stdout) are unaffected.

### Parallel tape (`demo/tapes/02-parallel-worktrees.tape`)
- Primarily re-render — realigned repo headers and the cleaner detail bar appear
  automatically in the dashboard + detail-tour shots.
- `Wait+Screen /bypass permissions on/` and the `za` detail-tour navigation are
  unaffected. Apply a small dwell tweak on the tour only if a take warrants it.

### Captions
- `demo/captions/01-hero.txt`: add one early beat naming the overlay (e.g.
  "Every workspace action lives behind Ctrl-x"); re-derive all start/end times
  against the new **post-ramp** clip (`01-hero-fast.mp4`).
- `demo/captions/02-parallel.txt`: re-derive times against the **collapsed**
  clip; optionally one beat for the cleaner detail bar.

### Timing re-tune (`demo/Makefile`) — all values take-specific
Adding the overlay dwell shifts every downstream timestamp, so these are
re-derived empirically from the new takes, not edited blind:
- `HERO_ADD_PROTECT` — agent-picker walk window (raw-clip seconds).
- `HERO_RAMP_START` / `HERO_RAMP_END` — Codex-churn ramp (collapsed-clip
  seconds); re-confirm `HERO_RAMP_FACTOR`.
- Re-confirm `MIN_FREEZE` / `MAX_HOLD` still collapse cleanly.

Tuning is done **stage-by-stage** (bootstrap → render → inspect raw → set
protect → deadair → inspect collapsed → set ramp → speedramp → set caption
times → post → verify ≤9MB budget), not one-shot `make`, so each window is set
against the actual frames.

## Execution order
1. **Parallel first** — cheaper, no live Codex coordination; validates the
   new-design render + dashboard/detail rendering end-to-end.
2. **Hero second** — expensive live Claude+Codex coordination; budget for 2–3
   takes (live-agent variance, per existing notes).

## Deliverable & handoff
- Commit harness changes (tapes, captions, Makefile, `SPIKE-NOTES.md`/README
  comment touch-ups for the overlay-dwell rule) as logical commits on
  `recreate-demo-screencasts`.
- Render both finals; hand both `.mp4`s to the user. The user uploads them to
  GitHub (web UI mints the `user-attachments/assets/...` URLs — no clean CLI
  path) and returns the two URLs.
- Commit the `README.md` URL swap: line 7 ← parallel, line 11 ← hero.
- Open the PR (never commit to `main`).

## Risks & mitigations
- **Live-agent variance (hero):** takes differ run-to-run. Mitigation: re-run a
  messy take; tune windows only against the take being shipped.
- **Toolchain:** vhs, ttyd, ffmpeg, chromium, agg, and logged-in `claude` +
  `codex` confirmed present in this environment.
- **9MB budget:** `post.sh` steps down fps/scale to fit; verify final sizes.
- **Sandbox isolation:** harness redirects all state into `$WSX_SANDBOX_ROOT`
  (`/tmp/wsx-demo`); only transient session-log symlinks touch `~/.claude`.
  `make -C demo clean` removes everything.

## Out of scope
- Re-storyboarding the clips or adding a third clip.
- Changes to the underlying TUI design (clips reflect `main` as-is).
- Hardening the pipeline against take-specific tuning (YAGNI).
