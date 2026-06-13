SHELL := /usr/bin/env bash
# Repo root (parent of this demo/ dir). Tapes write CWD-relative paths
# (demo/out/...), so every recipe runs from the root.
ROOT := $(realpath $(dir $(lastword $(MAKEFILE_LIST)))/..)
MIN_FREEZE := 5.0
MAX_HOLD := 1.3
# Hero: keep the "add a second agent harness" beat at full 1x. The selector marker
# stepping across the agent menu (claude · pi · hermes · codex) is too subtle for
# freezedetect, so deadair would otherwise collapse the whole walk. This is the
# raw-clip window of that beat — re-confirm if you re-record (see demo/SPIKE-NOTES.md).
HERO_ADD_PROTECT := 8.5:15.5
# Hero: speed-ramp Codex's fix/commit churn (deadair can't touch it — the screen
# is constantly changing). Span is absolute seconds in the collapsed clip, tuned
# to the recorded take (re-confirm if you re-record — see demo/SPIKE-NOTES.md).
HERO_RAMP_START := 44
HERO_RAMP_END := 69
HERO_RAMP_FACTOR := 3.5

.PHONY: all clips clean hero parallel check

all: clips
clips: hero parallel

clean:
	@root="$${WSX_SANDBOX_ROOT:-$${WSX_DEMO_ROOT:-/tmp/wsx-demo}}"; \
	root="$$(realpath -m -- "$$root")"; \
	case "$$root" in ""|/|/.|//|/tmp|"$$HOME") echo "refusing unsafe WSX_SANDBOX_ROOT='$$root'" >&2; exit 1;; esac; \
	case "$$root" in */*/*) : ;; *) echo "refusing too-shallow WSX_SANDBOX_ROOT='$$root'" >&2; exit 1;; esac; \
	rm -rf -- "$$root" $(ROOT)/demo/out; \
	: remove the transient session-log symlinks bridged into ~/.claude/projects; \
	find $$HOME/.claude/projects -maxdepth 1 -type l -lname "$$root/*" -delete 2>/dev/null || true

# Clip 1 — Hero: Claude reviews, delegates the fix to Codex over `wsx agent send`,
# Codex fixes + commits + reports back, Claude verifies — no human in the loop.
# Agents are launched via render.sh (clears Claude Code parent-session markers).
# Pipeline: render -> deadair (drop static air) -> speedramp (compress Codex's
# churn) -> post (caption + 9MB budget).
hero:
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash sandbox/render.sh demo/tapes/01-hero-multi-agent.tape
	cd $(ROOT) && bash demo/deadair.sh demo/out/01-hero-raw.mp4 demo/out/01-hero-collapsed.mp4 $(MIN_FREEZE) $(MAX_HOLD) 3.0 $(HERO_ADD_PROTECT)
	cd $(ROOT) && bash demo/speedramp.sh demo/out/01-hero-collapsed.mp4 demo/out/01-hero-fast.mp4 $(HERO_RAMP_START) $(HERO_RAMP_END) $(HERO_RAMP_FACTOR)
	cd $(ROOT) && bash demo/post.sh demo/out/01-hero-fast.mp4 demo/out/01-hero.mp4 demo/captions/01-hero.txt

# Clip 2 — three isolated worktrees, agents working in parallel (detail bars populate).
parallel:
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash sandbox/render.sh demo/tapes/02-parallel-worktrees.tape
	cd $(ROOT) && bash demo/deadair.sh demo/out/02-parallel-raw.mp4 demo/out/02-parallel-collapsed.mp4 $(MIN_FREEZE) $(MAX_HOLD)
	cd $(ROOT) && bash demo/post.sh demo/out/02-parallel-collapsed.mp4 demo/out/02-parallel.mp4 demo/captions/02-parallel.txt

# Unit checks for the scripted (non-recording) pieces.
check:
	cd $(ROOT)/demo && bash test-post.sh && bash test-speedramp.sh
