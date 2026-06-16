# Recreate wsx Demo Screencasts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recreate the two README-embedded demo screencasts (hero + parallel) so they reflect the current TUI design and deliberately feature its new elements (the `Ctrl-x` "actions" overlay, top workspace indicator, realigned dashboard headers, cleaner detail bar).

**Architecture:** Edit the existing VHS tapes and caption files to add showcase dwell beats, then re-record through the existing `demo/` pipeline (bootstrap → render → deadair → speedramp[hero] → post). Because the speed-ramp / dead-air-protect / caption windows are tuned per-take, they are re-derived empirically from each new recording by running the pipeline stage-by-stage and inspecting frames. Final `.mp4`s are handed to the user for GitHub upload; the README asset URLs are swapped once the user returns them.

**Tech Stack:** VHS (`.tape`), ttyd, ffmpeg/ffprobe, chromium (frame render), bash pipeline scripts, live `claude` + `codex` CLIs (sandboxed via `$WSX_SANDBOX_ROOT=/tmp/wsx-demo`).

**Spec:** `docs/superpowers/specs/2026-06-16-recreate-demo-screencasts-design.md`

---

## Notes for the implementer

- This is a **media-production** plan, not a code-feature plan. "Verification"
  means inspecting rendered artifacts (ffprobe durations, ≤9MB sizes, and
  reading the `Screenshot` PNGs the tapes emit), not unit tests. The only true
  unit tests here are `make -C demo check` (the scripted helpers) — run it after
  touching `post.sh`/`speedramp.sh`, but we are not changing those.
- **Run every command from the repo root** (`$ROOT`), not from `demo/` — the
  tapes write CWD-relative `demo/out/...` paths. The Makefile enforces this; the
  manual stage commands below do too.
- **Pipeline script signatures** (confirmed):
  - `bash sandbox/render.sh <tape>` → writes the tape's `Output` path.
  - `bash demo/deadair.sh IN OUT MIN_FREEZE MAX_HOLD TAIL_PROTECT [START:END protect]`
  - `bash demo/speedramp.sh IN OUT START_S END_S FACTOR`
  - `bash demo/post.sh IN OUT CAPTIONS_TSV`
- **Why parallel first:** it has no live Codex coordination, so it's the cheaper,
  more deterministic clip — it validates that the new TUI chrome renders cleanly
  through the whole pipeline before spending the expensive hero takes.
- **Sandbox safety:** everything lands in `/tmp/wsx-demo`; `make -C demo clean`
  wipes it plus the transient `~/.claude/projects` symlinks. Never commit
  anything under `demo/out/` (gitignored).
- **Commit discipline:** never commit to `main`; we are on
  `recreate-demo-screencasts`. Commit messages use Conventional Commits.

---

## Task 1: Add the overlay-showcase dwell beat to the hero tape

**Files:**
- Modify: `demo/tapes/01-hero-multi-agent.tape` (the first `Ctrl+x`, ~line 38)

- [ ] **Step 1: Insert a readable dwell on the new "actions" overlay**

In `demo/tapes/01-hero-multi-agent.tape`, the "Put a second agent (Codex)"
section currently reads:

```
Ctrl+x
Type "a"
Sleep 2500ms          # hold on the open picker — all four harnesses on screen
```

Change it to dwell on the new overlay first, then open the picker:

```
Ctrl+x
Sleep 1800ms          # NEW: hold on the centered "actions" overlay so it reads
                      # (static screen, < MIN_FREEZE 5s, so deadair won't collapse it)
Type "a"
Sleep 2500ms          # hold on the open picker — all four harnesses on screen
```

Leave the later `Ctrl+x`/`Type "w"`, `Ctrl+x`/`Type "q"`, `Ctrl+x`/`Type "d"`
beats unchanged — `w`/`q` are pane-cycle keys (not overlay rows), so they stay
quick.

- [ ] **Step 2: Sanity-check the tape still parses**

Run: `vhs validate demo/tapes/01-hero-multi-agent.tape`
Expected: no errors (exit 0). If `vhs validate` is unavailable, confirm the edit
is syntactically a `Sleep` line between `Ctrl+x` and `Type "a"`.

- [ ] **Step 3: Commit**

```bash
git add demo/tapes/01-hero-multi-agent.tape
git commit -m "demo(hero): dwell on the new Ctrl-x actions overlay before adding Codex"
```

---

## Task 2: Add the hero caption beat for the overlay

**Files:**
- Modify: `demo/captions/01-hero.txt`

- [ ] **Step 1: Add an early caption naming the overlay**

`demo/captions/01-hero.txt` is tab-separated `start<TAB>end<TAB>text` in seconds,
timed against the **post-ramp** clip (`01-hero-fast.mp4`). The first line is
currently:

```
0.5	5	Spin up a workspace right from the dashboard
```

Insert a beat for the overlay after the workspace spin-up and before the
"Add any agent harness" line. Use placeholder times now — Task 6 re-derives ALL
times against the real take, so exact values here are provisional:

```
0.5	5	Spin up a workspace right from the dashboard
6	9	Every workspace action lives behind Ctrl-x
```

Then shift the subsequent lines' provisional start/end later by ~3s so they
don't overlap (final values come from Task 6). The caption text wording is the
deliverable here; the timings are placeholders.

- [ ] **Step 2: Verify the file is still valid TSV**

Run: `awk -F'\t' 'NF!=3{print "BAD LINE "NR": "$0; bad=1} END{exit bad}' demo/captions/01-hero.txt && echo "OK 3-column TSV"`
Expected: `OK 3-column TSV` (every line has exactly 3 tab-separated fields).

- [ ] **Step 3: Commit**

```bash
git add demo/captions/01-hero.txt
git commit -m "demo(hero): caption the Ctrl-x actions overlay (timings provisional)"
```

---

## Task 3: Record + tune the PARALLEL clip (do this clip first)

**Files:**
- Modify (timings only): `demo/Makefile`, `demo/captions/02-parallel.txt`
- Read (inspect): `demo/out/02-*.png`, `demo/out/02-*.mp4`

- [ ] **Step 1: Bootstrap the isolated sandbox + record the raw take**

```bash
bash sandbox/bootstrap.sh
bash sandbox/render.sh demo/tapes/02-parallel-worktrees.tape
```

Expected: `demo/out/02-parallel-raw.mp4` exists. (~2–4 min; spins up 3 live
agents in the sandbox.)

Run: `ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/02-parallel-raw.mp4`
Expected: a duration > 90s prints.

- [ ] **Step 2: Confirm the NEW design rendered (read the screenshots)**

Read these PNGs and confirm the redesigned chrome is present and not garbled:
- `demo/out/02-modal.png` — new-workspace modal.
- `demo/out/02-dashboard.png` and `demo/out/02-dashboard-done.png` — realigned
  repo headers (right-justified names, flush-right paths, rule-filled left-pad).
- `demo/out/02-detail-1.png`, `02-detail-2.png`, `02-detail-3.png` — detail bar
  with module scrollbars HIDDEN and the reclaimed column (SESSION SUMMARY /
  RECENT CHAT / RECENT FILES with `+X −Y` counts).

Expected: agents finished (3 commits), detail bars populated, no visual
corruption. If a take is messy, `make -C demo clean` and re-run Step 1.

- [ ] **Step 3: Collapse dead air**

```bash
bash demo/deadair.sh demo/out/02-parallel-raw.mp4 demo/out/02-parallel-collapsed.mp4 5.0 1.3
```

Run: `ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/02-parallel-collapsed.mp4`
Expected: duration noticeably shorter than the raw (the agent-boot/settle holds
collapsed). Target ~55–65s. If too aggressive (active content got clipped),
raise `MIN_FREEZE`; if too loose, lower it — but the defaults usually hold for
this clip.

- [ ] **Step 4: Re-derive caption timings against the collapsed clip**

Scrub the collapsed clip to find the real second-offsets of the three beats
(modal create, deploy-to-each, detail-bar tour). Extract probe frames to time
them precisely:

```bash
for t in 5 15 25 35 45 55; do ffmpeg -y -loglevel error -ss $t -i demo/out/02-parallel-collapsed.mp4 -frames:v 1 /tmp/p-$t.png; done
```

Read `/tmp/p-*.png`, note which second each beat starts, and update
`demo/captions/02-parallel.txt` start/end times to match. Keep the three beats:
worktree creation, deploy-to-each, detail-bar (`+/−` counts). Optionally add a
beat naming the cleaner detail bar if the tour has room.

Run: `awk -F'\t' 'NF!=3{print "BAD "NR; bad=1} END{exit bad}' demo/captions/02-parallel.txt && echo OK`
Expected: `OK`.

- [ ] **Step 5: Burn in captions + enforce the size budget**

```bash
bash demo/post.sh demo/out/02-parallel-collapsed.mp4 demo/out/02-parallel.mp4 demo/captions/02-parallel.txt
```

Run: `ls -l demo/out/02-parallel.mp4 && ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/02-parallel.mp4`
Expected: file ≤ ~9MB (9_437_184 bytes), captions present.

- [ ] **Step 6: Final visual review**

Read a few sampled frames of `demo/out/02-parallel.mp4` over the captioned spans:

```bash
for t in 3 18 50; do ffmpeg -y -loglevel error -ss $t -i demo/out/02-parallel.mp4 -frames:v 1 /tmp/pf-$t.png; done
```

Read `/tmp/pf-*.png` and confirm each caption is on screen at the right beat and
the new chrome looks right.

- [ ] **Step 7: Commit the parallel tuning**

```bash
git add demo/captions/02-parallel.txt demo/Makefile
git commit -m "demo(parallel): re-tune caption timings + dead-air for the new dashboard design"
```

(Only `demo/Makefile` changes if you adjusted `MIN_FREEZE`/`MAX_HOLD`; if you
didn't, omit it from the `git add`.)

---

## Task 4: Record the HERO raw take + set the dead-air protect window

**Files:**
- Modify (timings only): `demo/Makefile` (`HERO_ADD_PROTECT`)
- Read (inspect): `demo/out/01-*.png`, `demo/out/01-hero-raw.mp4`

- [ ] **Step 1: Bootstrap + record the raw hero take**

```bash
bash sandbox/bootstrap.sh
bash sandbox/render.sh demo/tapes/01-hero-multi-agent.tape
```

Expected: `demo/out/01-hero-raw.mp4` exists (~3–6 min; live Claude reviews,
delegates over `wsx agent send`, Codex fixes+commits+reports, Claude verifies).
If the cross-agent relay didn't happen (no `queued message to codex`, or Codex
never replied), `make -C demo clean` and re-record — live variance, budget 2–3
takes.

- [ ] **Step 2: Confirm the new chrome + the overlay dwell landed**

Read the tape's screenshots:
- `demo/out/01-agents.png` — agent picker with Codex selected; the top
  workspace indicator + separator rule + `^x` chip hint should be visible.
- `demo/out/01-claude-delegates.png`, `01-codex-handoff.png`,
  `01-codex-reports.png`, `01-claude-verifies.png`, `01-outro.png`.

Then verify the **actions overlay** is actually on screen during the dwell.
Find its raw-clip timestamp (the dwell is right before the picker opens):

```bash
for t in 6 7 8 9 10 11; do ffmpeg -y -loglevel error -ss $t -i demo/out/01-hero-raw.mp4 -frames:v 1 /tmp/h-$t.png; done
```

Read `/tmp/h-*.png` and note the second range where the centered bordered
"actions" box (rows `d/u/a/e/t/v/g/k/x`) is visible. Record that window.

- [ ] **Step 3: Set the agent-picker protect window**

`HERO_ADD_PROTECT` in `demo/Makefile` keeps the agent-picker selector walk at 1×
(its motion is too subtle for `freezedetect`). Re-derive it: from the raw-clip
frames, find the start (overlay appears / picker opens) and end (selector lands
on Codex, just before `Enter`). Note the overlay dwell is now part of this beat,
so the window likely **starts ~1.8s earlier** than the old `8.5:15.5`.

Update the Makefile line, e.g.:

```makefile
HERO_ADD_PROTECT := 6.5:17.0
```

Use the actual start:end seconds you measured (raw-clip seconds).

- [ ] **Step 4: Collapse dead air with the protect window**

```bash
bash demo/deadair.sh demo/out/01-hero-raw.mp4 demo/out/01-hero-collapsed.mp4 5.0 1.3 3.0 "$(make -s -C demo print-HERO_ADD_PROTECT 2>/dev/null || echo 6.5:17.0)"
```

If the `make print-` helper doesn't exist, pass the literal window you set:

```bash
bash demo/deadair.sh demo/out/01-hero-raw.mp4 demo/out/01-hero-collapsed.mp4 5.0 1.3 3.0 6.5:17.0
```

Run: `ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/01-hero-collapsed.mp4`
Expected: duration prints; the picker-walk beat is intact (re-check by sampling a
frame mid-window). Note this duration — Task 5 needs it.

- [ ] **Step 5: Commit the protect window**

```bash
git add demo/Makefile
git commit -m "demo(hero): re-tune dead-air protect window for the overlay-dwell take"
```

---

## Task 5: Set the hero speed-ramp window against the collapsed clip

**Files:**
- Modify (timings only): `demo/Makefile` (`HERO_RAMP_START`, `HERO_RAMP_END`, `HERO_RAMP_FACTOR`)
- Read (inspect): `demo/out/01-hero-collapsed.mp4`

- [ ] **Step 1: Locate Codex's fix/commit churn in the collapsed clip**

The ramp speeds up the one long actively-changing span deadair can't touch
(Codex churning through its fix + commit). Find its start (Codex starts working
after the handoff `Enter`) and end (Codex prints its commit hash / reports back)
in **collapsed-clip seconds**:

```bash
for t in 30 35 40 45 50 55 60 65 70; do ffmpeg -y -loglevel error -ss $t -i demo/out/01-hero-collapsed.mp4 -frames:v 1 /tmp/c-$t.png; done
```

Read `/tmp/c-*.png`, identify the churn span. Do NOT ramp over reading-critical
beats (Claude's finding, the handoff message, Codex's report, Claude's verify).

- [ ] **Step 2: Update the ramp window**

Edit `demo/Makefile` with the measured collapsed-clip seconds (old values were
`44`/`69`/`3.5` — expect a shift from the upstream overlay dwell):

```makefile
HERO_RAMP_START := <measured start, collapsed-clip s>
HERO_RAMP_END := <measured end, collapsed-clip s>
HERO_RAMP_FACTOR := 3.5
```

- [ ] **Step 3: Apply the ramp**

```bash
bash demo/speedramp.sh demo/out/01-hero-collapsed.mp4 demo/out/01-hero-fast.mp4 <START> <END> 3.5
```

Run: `ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/01-hero-fast.mp4`
Expected: shorter than collapsed; target a final clip ~60–70s. If the churn
still drags, raise `HERO_RAMP_FACTOR`; if a beat got compressed, narrow the
window.

- [ ] **Step 4: Commit the ramp window**

```bash
git add demo/Makefile
git commit -m "demo(hero): re-tune speed-ramp window for the new take"
```

---

## Task 6: Finalize hero captions + render the captioned hero clip

**Files:**
- Modify: `demo/captions/01-hero.txt` (final timings)
- Read (inspect): `demo/out/01-hero-fast.mp4`, `demo/out/01-hero.mp4`

- [ ] **Step 1: Re-derive every caption time against the post-ramp clip**

Captions are timed against `01-hero-fast.mp4` (the post-ramp clip `post.sh`
actually captions). Sample frames across the whole clip and map each beat to its
real second-offset:

```bash
D=$(ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/01-hero-fast.mp4); echo "dur=$D"
for t in $(seq 2 4 ${D%.*}); do ffmpeg -y -loglevel error -ss $t -i demo/out/01-hero-fast.mp4 -frames:v 1 /tmp/hf-$t.png; done
```

Read `/tmp/hf-*.png` and set start/end for each line in
`demo/captions/01-hero.txt`, including the new overlay beat from Task 2. Keep the
narrative arc: spin up → Ctrl-x overlay → add Codex → Claude finds the bug →
hands off over the wsx CLI → Codex receives context → applies fix + commits →
reports back → Claude verifies → "two agents coordinated end to end".

Run: `awk -F'\t' 'NF!=3{print "BAD "NR; bad=1} END{exit bad}' demo/captions/01-hero.txt && echo OK`
Expected: `OK`.

- [ ] **Step 2: Burn in captions + enforce the budget**

```bash
bash demo/post.sh demo/out/01-hero-fast.mp4 demo/out/01-hero.mp4 demo/captions/01-hero.txt
```

Run: `ls -l demo/out/01-hero.mp4 && ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/01-hero.mp4`
Expected: ≤ ~9MB, captions burned in.

- [ ] **Step 3: Final visual review of the hero clip**

```bash
for t in 7 20 40 60; do ffmpeg -y -loglevel error -ss $t -i demo/out/01-hero.mp4 -frames:v 1 /tmp/hfin-$t.png; done
```

Read `/tmp/hfin-*.png`: confirm the overlay caption aligns with the overlay
on-screen, the handoff/verify beats are readable, and captions match the action.

- [ ] **Step 4: Commit final hero captions**

```bash
git add demo/captions/01-hero.txt
git commit -m "demo(hero): finalize caption timings against the re-recorded take"
```

---

## Task 7: Document the new overlay-dwell tuning rule

**Files:**
- Modify: `demo/SPIKE-NOTES.md`, `demo/README.md`

- [ ] **Step 1: Record the overlay-dwell gotcha in SPIKE-NOTES**

Add a short note to `demo/SPIKE-NOTES.md` capturing the new mechanic: the
attached view's keybind footer was replaced by a centered `Ctrl-x` "actions"
overlay; to showcase it, dwell on it after `Ctrl+x` but keep the dwell **under
`MIN_FREEZE` (5s)** so `deadair` doesn't collapse it (or add it to
`HERO_ADD_PROTECT`). Note that `w`/`q` pane-cycle keys are not overlay rows, so
they stay quick.

- [ ] **Step 2: Refresh README take-specific value mentions**

In `demo/README.md`, the "Customizing" section references `HERO_ADD_PROTECT`
"(8.5:15.5)" and the ramp window as take-specific. Update any hard-coded example
values to the newly-tuned ones (or generalize them so they don't drift), and add
the overlay-dwell beat to the hero clip description if it lists beats.

- [ ] **Step 3: Commit the docs**

```bash
git add demo/SPIKE-NOTES.md demo/README.md
git commit -m "docs(demo): record the Ctrl-x overlay-dwell tuning rule"
```

---

## Task 8: Hand off the clips + swap the README asset URLs + open the PR

**Files:**
- Modify: `README.md` (lines 7 and 11)

- [ ] **Step 1: Verify both finals meet budget**

```bash
ls -l demo/out/01-hero.mp4 demo/out/02-parallel.mp4
for f in demo/out/01-hero.mp4 demo/out/02-parallel.mp4; do ffprobe -v error -show_entries format=duration -of csv=p=0 "$f"; done
```

Expected: both ≤ ~9MB; hero ~60–70s, parallel ~55–65s.

- [ ] **Step 2: Send both clips to the user for GitHub upload**

Use `SendUserFile` to deliver `demo/out/01-hero.mp4` and
`demo/out/02-parallel.mp4`, with a caption telling the user: drag each into a
GitHub PR/issue comment to mint the `user-attachments/assets/...` URL, then send
back the two URLs (note which is hero, which is parallel).

- [ ] **Step 3: Swap the README asset URLs (after the user returns them)**

In `README.md`:
- Line 7 (under "Parallel Agent Sessions") ← the **parallel** clip URL.
- Line 11 (under "Multi Agent Sessions") ← the **hero** clip URL.

Replace only the two `https://github.com/user-attachments/assets/...` lines.

- [ ] **Step 4: Commit the README swap**

```bash
git add README.md
git commit -m "docs: point README screencasts at the re-recorded clips"
```

- [ ] **Step 5: Sandbox cleanup**

```bash
make -C demo clean
```

Expected: `/tmp/wsx-demo` and `demo/out/` removed, transient `~/.claude/projects`
symlinks deleted.

- [ ] **Step 6: Push the branch and open the PR**

Use the `pull-request` skill (or `gh pr create`) against `main`. The PR body
should summarize: both clips re-recorded under the new design, overlay
showcased, timings re-tuned, README URLs swapped. Confirm CI (rustfmt/clippy/test
gates) is unaffected — this branch touches only `demo/`, `docs/`, and
`README.md`.

---

## Self-Review (completed by author)

- **Spec coverage:** hero overlay dwell (T1–T2, T6), parallel re-render under new
  dashboard/detail design (T3), hero protect/ramp/caption re-tune (T4–T6), docs
  for the overlay-dwell rule (T7), mp4 handoff + README URL swap + PR (T8),
  parallel-first ordering (T3 before T4), stage-by-stage tuning (T3–T6),
  sandbox cleanup (T8). All spec sections map to a task.
- **Placeholder scan:** the only intentionally-deferred values are the
  take-specific timing windows (protect/ramp/caption seconds) — by design these
  are *measured from the recorded take* in their own steps, with explicit
  measurement commands, not left vague.
- **Consistency:** script arg orders match the confirmed signatures; clip→README
  line mapping (parallel=line 7, hero=line 11) is consistent throughout; the
  caption-timing reference clip (hero=post-ramp `01-hero-fast.mp4`,
  parallel=collapsed) matches the spec and the Makefile pipeline.
