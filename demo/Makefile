SHELL := /usr/bin/env bash
# Repo root (parent of this demo/ dir). Tapes write CWD-relative paths
# (demo/out/...), so every recipe runs from the root.
ROOT := $(realpath $(dir $(lastword $(MAKEFILE_LIST)))/..)
MIN_FREEZE := 5.0
MAX_HOLD := 1.3

.PHONY: all clips clean hero parallel check

all: clips
clips: hero parallel

clean:
	rm -rf $${WSX_DEMO_ROOT:-/tmp/wsx-demo} $(ROOT)/demo/out
	@# remove the transient session-log symlinks bridged into ~/.claude/projects
	find $$HOME/.claude/projects -maxdepth 1 -type l -lname "$${WSX_DEMO_ROOT:-/tmp/wsx-demo}/*" -delete 2>/dev/null || true

# Clip 1 — Claude reviews, Codex fixes, on one workspace.
# Agents are launched via render.sh (clears Claude Code parent-session markers).
hero:
	cd $(ROOT) && bash demo/sandbox-bootstrap.sh
	cd $(ROOT) && bash demo/render.sh demo/tapes/01-hero-multi-agent.tape
	cd $(ROOT) && bash demo/deadair.sh demo/out/01-hero-raw.mp4 demo/out/01-hero-collapsed.mp4 $(MIN_FREEZE) $(MAX_HOLD)
	cd $(ROOT) && bash demo/post.sh demo/out/01-hero-collapsed.mp4 demo/out/01-hero.mp4 demo/captions/01-hero.txt

# Clip 2 — three isolated worktrees, agents working in parallel (detail bars populate).
parallel:
	cd $(ROOT) && bash demo/sandbox-bootstrap.sh
	cd $(ROOT) && bash demo/render.sh demo/tapes/02-parallel-worktrees.tape
	cd $(ROOT) && bash demo/deadair.sh demo/out/02-parallel-raw.mp4 demo/out/02-parallel-collapsed.mp4 $(MIN_FREEZE) $(MAX_HOLD)
	cd $(ROOT) && bash demo/post.sh demo/out/02-parallel-collapsed.mp4 demo/out/02-parallel.mp4 demo/captions/02-parallel.txt

# Unit checks for the scripted (non-recording) pieces.
check:
	cd $(ROOT)/demo && bash test-gen-repos.sh && bash test-bootstrap.sh && bash test-post.sh
