SHELL := /usr/bin/env bash
# Repo root (parent of this demo/ dir). The scripts write CWD-relative paths
# (demo/out/...), so every recipe runs from the root.
ROOT := $(realpath $(dir $(lastword $(MAKEFILE_LIST)))/..)

.PHONY: all clips clean hero parallel hero-still agent-chat-still check

all: clips
clips: hero parallel

clean:
	@root="$${WSX_SANDBOX_ROOT:-$${WSX_DEMO_ROOT:-/tmp/wsx-demo}}"; \
	root="$$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$$root")"; \
	case "$$root" in ""|/|/.|//|/tmp|"$$HOME") echo "refusing unsafe WSX_SANDBOX_ROOT='$$root'" >&2; exit 1;; esac; \
	case "$$root" in */*/*) : ;; *) echo "refusing too-shallow WSX_SANDBOX_ROOT='$$root'" >&2; exit 1;; esac; \
	rm -rf -- "$$root" $(ROOT)/demo/out; \
	: remove the transient session-log symlinks bridged into ~/.claude/projects; \
	find $$HOME/.claude/projects -maxdepth 1 -type l -lname "$$root/*" -delete 2>/dev/null || true

# Clip 1 — Hero: Claude reviews, delegates the fix to Codex over `wsx agent send`,
# Codex fixes + commits + reports back, Claude verifies — no human in the loop.
# tmux drives the TUI; demo/clips/render.sh turns the capture into the MP4.
hero:
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash demo/clips/01-hero.sh

# Clip 2 — three isolated worktrees, agents working in parallel (detail bars
# populate), then one workspace tagged and hidden behind a #parked group row.
parallel:
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash demo/clips/02-parallel.sh

# Still — the dashboard under load (site hero image). Three repos, nine
# workspaces, Claude + Codex on one of them, recaps inline and in the detail
# bar, tags and #parked hidden groups; the last takes open the F drawer.
# Seeds everything from the CLI, then tmux + Chrome screenshot the dashboard.
hero-still:
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash demo/stills/dashboard-hero.sh

# Still — the attached view: Claude hands a fix to Codex over `wsx agent send`,
# Codex reports back, pinned-command chips and the agents row along the bottom.
agent-chat-still:
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash demo/stills/agent-chat.sh

# Unit checks for the scripted (non-recording) pieces.
check:
	cd $(ROOT)/demo && bash clips/test-timeline.sh
