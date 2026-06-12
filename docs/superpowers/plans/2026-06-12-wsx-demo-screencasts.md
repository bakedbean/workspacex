# wsx Demo-Screencast Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A committed, re-runnable harness under `demo/` that stands up an isolated wsx install with synthetic repos, drives live Claude+Codex+Pi reviews via VHS, and renders captioned landscape clips under a hard 10MB budget.

**Architecture:** Bash scripts orchestrate an `XDG_STATE_HOME`-isolated wsx sandbox; `gen-repos.sh` builds synthetic git repos with planted bugs; VHS `.tape` files drive the real `wsx` TUI with live agents (using `Wait` to block on agent completion); `post.sh` runs an ffmpeg captions + budget-enforcement pass; a `Makefile` chains it all.

**Tech Stack:** Bash, git, VHS (`charmbracelet/vhs`) + ttyd, ffmpeg, agg (GIF), the installed `wsx` binary on PATH, live `claude`/`codex`/`pi`.

**Spec:** `docs/superpowers/specs/2026-06-12-wsx-demo-screencasts-design.md`

**Conventions for every task below:**
- All `wsx`/agent commands run with `XDG_STATE_HOME` pointed at the sandbox. Scripts `export XDG_STATE_HOME` near the top; never rely on the caller's env.
- Sandbox root defaults to `WSX_DEMO_ROOT=/tmp/wsx-demo` (overridable).
- Demo artifacts live under `demo/`; generated output under `demo/out/` (gitignored).
- Commit after each task. Branch is `demo-screencasts` (never commit to main).
- `demo/*.sh` are `#!/usr/bin/env bash` + `set -euo pipefail` and pass `shellcheck`.

---

### Task 1: De-risk smoke spike — VHS drives wsx with a live agent

**This task gates the rest.** Its job is to answer two questions before we invest in real tapes:
1. Can VHS launch the `wsx` TUI, drive it with keystrokes, spawn a *live* agent inside it, and have the agent's interactive TUI render correctly through nested PTYs (VHS → tmux → wsx → agent)?
2. Does `Wait+Screen /regex/` reliably fire when real agent output appears, so the tape blocks on completion instead of racing?

**Files:**
- Create: `demo/.gitignore` (contents: `out/`)
- Create: `demo/tapes/spike.tape`
- Create: `demo/SPIKE-NOTES.md` (records the resolved driving method)

- [ ] **Step 1: Install the recording toolchain**

Run:
```bash
which vhs ttyd agg || true
# Arch / omarchy:
sudo pacman -S --needed ttyd ffmpeg
# vhs and agg are not in core repos; install via go or release binary:
go install github.com/charmbracelet/vhs@latest 2>/dev/null || \
  echo "fallback: download vhs release binary from github.com/charmbracelet/vhs/releases"
go install github.com/asciinema/agg@latest 2>/dev/null || \
  echo "fallback: download agg release binary"
```
Expected: `vhs --version`, `ttyd --version`, `ffmpeg -version`, `agg --version` all succeed. If `vhs` cannot be installed, STOP and report — the plan's capture choice must be revisited (fallback: asciinema, per spec De-risking section).

- [ ] **Step 2: Write the spike tape (minimal, throwaway)**

Create `demo/tapes/spike.tape`:
```tape
# Spike: prove VHS can drive wsx + a live agent through nested PTYs.
Output demo/out/spike.mp4
Set Shell "bash"
Set FontSize 16
Set Width 1280
Set Height 720
Set TypingSpeed 60ms

# Isolated sandbox already bootstrapped by the spike runner (see Step 3).
Env XDG_STATE_HOME "/tmp/wsx-demo/state"

Type "wsx"
Enter
Sleep 3s            # dashboard renders
Screenshot demo/out/spike-dashboard.png

# Create a workspace with a live claude agent attached.
Type "n"            # new workspace (adjust to real keybinding from README if 'n' differs)
Sleep 1s
# ... select repo / confirm slug — exact keys filled in during the spike ...
Enter
Sleep 5s
Screenshot demo/out/spike-agent-spawned.png

# Type a narrow review prompt into the agent pane and wait for it to finish.
Type "Review src/auth.py and report the single most serious bug in one sentence."
Enter
Wait+Screen /(bug|issue|vulnerab|finding)/
Sleep 2s
Screenshot demo/out/spike-agent-done.png

Type "q"           # or Ctrl-x d to detach, then quit wsx
Sleep 1s
```

- [ ] **Step 3: Bootstrap a minimal sandbox for the spike**

Run (inline, not yet scripted — gen-repos comes in Task 2):
```bash
export WSX_DEMO_ROOT=/tmp/wsx-demo
export XDG_STATE_HOME="$WSX_DEMO_ROOT/state"
rm -rf "$WSX_DEMO_ROOT"; mkdir -p "$XDG_STATE_HOME" "$WSX_DEMO_ROOT/repos/toy-api/src"
cd "$WSX_DEMO_ROOT/repos/toy-api"
git init -q && git config user.email demo@wsx.dev && git config user.name "wsx demo"
printf 'def login(user, pw):\n    # BUG: auth bypass — always returns True\n    return True\n' > src/auth.py
git add -A && git commit -qm "initial toy-api"
wsx repo add "$WSX_DEMO_ROOT/repos/toy-api" --name toy-api
```
Expected: `wsx repo list` shows `toy-api`, and `ls "$XDG_STATE_HOME/wsx/state.db"` exists (proves isolation — nothing written under the real `~/.local/state`).

- [ ] **Step 4: Run the spike tape and inspect**

Run: `vhs demo/tapes/spike.tape`
Expected: `demo/out/spike.mp4` is produced; the three screenshots show (a) the dashboard, (b) a live agent pane, (c) agent output containing the finding. Watch the mp4 — confirm the agent TUI rendered (not garbled) and that `Wait+Screen` actually paused for the live agent.

- [ ] **Step 5: Record the resolved driving method**

Write `demo/SPIKE-NOTES.md` capturing the ground truth discovered:
- Exact keybinding sequence to create a workspace + attach an agent in the TUI (corrected against actual behavior — the README lists `n` for new workspace; verify).
- Whether prompts must be typed into the agent pane (TUI driving) or can be pre-seeded; how `Wait+Screen` regexes behaved; any nested-PTY rendering quirks and their fixes (e.g. `Set Framerate`, font, `TERM`).
- If VHS nesting failed: document the asciinema fallback decision and switch subsequent tape tasks to `.cast` capture.

- [ ] **Step 6: Commit**

```bash
git add demo/.gitignore demo/tapes/spike.tape demo/SPIKE-NOTES.md
git commit -m "demo: VHS+live-agent smoke spike for screencast harness"
```

---

### Task 2: Synthetic repo generator

**Files:**
- Create: `demo/gen-repos.sh`

- [ ] **Step 1: Write a failing check**

Create `demo/test-gen-repos.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
"$(dirname "$0")/gen-repos.sh" "$tmp"
test -d "$tmp/toy-api/.git" || { echo "FAIL: toy-api not a git repo"; exit 1; }
test -d "$tmp/toy-cli/.git" || { echo "FAIL: toy-cli not a git repo"; exit 1; }
grep -rq "BUG:" "$tmp/toy-api" || { echo "FAIL: no planted bug in toy-api"; exit 1; }
git -C "$tmp/toy-api" rev-parse HEAD >/dev/null || { echo "FAIL: no commit"; exit 1; }
echo "PASS"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `bash demo/test-gen-repos.sh`
Expected: FAIL — `gen-repos.sh: No such file or directory`.

- [ ] **Step 3: Implement `demo/gen-repos.sh`**

```bash
#!/usr/bin/env bash
# Generate small synthetic repos with deliberately planted, reviewable bugs.
# Usage: gen-repos.sh <dest-dir>
set -euo pipefail
DEST="${1:?usage: gen-repos.sh <dest-dir>}"
mkdir -p "$DEST"

init_repo() { # <path>
  git -C "$1" init -q
  git -C "$1" config user.email demo@wsx.dev
  git -C "$1" config user.name "wsx demo"
}

# --- toy-api: a tiny Flask-style service with planted security bugs ---
API="$DEST/toy-api"; mkdir -p "$API/src"
cat > "$API/src/auth.py" <<'PY'
import sqlite3

def login(username, password):
    # BUG: SQL injection — username/password interpolated into the query.
    q = f"SELECT * FROM users WHERE name='{username}' AND pw='{password}'"
    return sqlite3.connect("app.db").execute(q).fetchone()

def is_admin(token):
    # BUG: auth bypass — any non-empty token is treated as admin.
    return bool(token)
PY
cat > "$API/src/app.py" <<'PY'
from src.auth import login, is_admin

def handle(req):
    user = login(req["user"], req["pw"])
    # BUG: unhandled None — login() returns None on bad creds, then .id crashes.
    return {"id": user.id, "admin": is_admin(req.get("token"))}
PY
cat > "$API/README.md" <<'MD'
# toy-api
A minimal example service used for wsx demo recordings.
MD
init_repo "$API"
git -C "$API" add -A
git -C "$API" commit -qm "feat: initial toy-api service"

# --- toy-cli: a small CLI with planted correctness/resource bugs ---
CLI="$DEST/toy-cli"; mkdir -p "$CLI/src"
cat > "$CLI/src/main.py" <<'PY'
import sys

def parse_args(argv):
    # BUG: off-by-one — skips the first real argument.
    return argv[2:]

def read_config(path):
    # BUG: file handle leaked — never closed.
    f = open(path)
    return f.read()

def main():
    args = parse_args(sys.argv)
    print(read_config(args[0]))

if __name__ == "__main__":
    main()
PY
cat > "$CLI/README.md" <<'MD'
# toy-cli
A minimal example CLI used for wsx demo recordings.
MD
init_repo "$CLI"
git -C "$CLI" add -A
git -C "$CLI" commit -qm "feat: initial toy-cli"

echo "generated repos in $DEST"
```

- [ ] **Step 4: Run the check to verify it passes**

Run: `chmod +x demo/gen-repos.sh demo/test-gen-repos.sh && bash demo/test-gen-repos.sh`
Expected: `PASS`.

- [ ] **Step 5: Commit**

```bash
git add demo/gen-repos.sh demo/test-gen-repos.sh
git commit -m "demo: synthetic repo generator with planted review bugs"
```

---

### Task 3: Sandbox bootstrap

**Files:**
- Create: `demo/sandbox-bootstrap.sh`

- [ ] **Step 1: Write a failing check**

Create `demo/test-bootstrap.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
export WSX_DEMO_ROOT="$(mktemp -d)/wsx-demo"
trap 'rm -rf "$WSX_DEMO_ROOT"' EXIT
"$(dirname "$0")/sandbox-bootstrap.sh"
test -f "$WSX_DEMO_ROOT/state/wsx/state.db" || { echo "FAIL: no isolated db"; exit 1; }
XDG_STATE_HOME="$WSX_DEMO_ROOT/state" wsx repo list | grep -q toy-api || { echo "FAIL: toy-api not registered"; exit 1; }
XDG_STATE_HOME="$WSX_DEMO_ROOT/state" wsx repo list | grep -q toy-cli || { echo "FAIL: toy-cli not registered"; exit 1; }
echo "PASS"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `bash demo/test-bootstrap.sh`
Expected: FAIL — `sandbox-bootstrap.sh: No such file or directory`.

- [ ] **Step 3: Implement `demo/sandbox-bootstrap.sh`**

```bash
#!/usr/bin/env bash
# Stand up an isolated wsx install with synthetic repos registered.
# Everything lives under $WSX_DEMO_ROOT; the real ~/.local/state is untouched.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export WSX_DEMO_ROOT="${WSX_DEMO_ROOT:-/tmp/wsx-demo}"
export XDG_STATE_HOME="$WSX_DEMO_ROOT/state"
REPOS="$WSX_DEMO_ROOT/repos"

# Fresh state each run.
rm -rf "$WSX_DEMO_ROOT"
mkdir -p "$XDG_STATE_HOME" "$REPOS"

# Generate and register repos.
"$HERE/gen-repos.sh" "$REPOS"
wsx repo add "$REPOS/toy-api" --name toy-api --prefix demo
wsx repo add "$REPOS/toy-cli" --name toy-cli --prefix demo

echo "sandbox ready at $WSX_DEMO_ROOT (XDG_STATE_HOME=$XDG_STATE_HOME)"
```

Note: `wsx repo add` creates `state.db` on first write. If the very first `wsx`
call does not create the db, add a `wsx repo list >/dev/null` before the asserts.

- [ ] **Step 4: Run the check to verify it passes**

Run: `chmod +x demo/sandbox-bootstrap.sh demo/test-bootstrap.sh && bash demo/test-bootstrap.sh`
Expected: `PASS`.

- [ ] **Step 5: Verify real state is untouched**

Run: `ls -la ~/.local/state/wsx/state.db && git -C ~/.local/state/wsx 2>/dev/null; echo "real db mtime unchanged expected"`
Expected: the real db (if any) is not modified by the bootstrap. (Sanity check only — no assertion.)

- [ ] **Step 6: Commit**

```bash
git add demo/sandbox-bootstrap.sh demo/test-bootstrap.sh
git commit -m "demo: isolated sandbox bootstrap"
```

---

### Task 4: Hero tape — multi-agent review on one workspace (Clip 1)

Uses the driving method resolved in `demo/SPIKE-NOTES.md`. The tape below is the
first draft; correct keybindings/sleeps against the spike notes.

**Files:**
- Create: `demo/tapes/01-hero-multi-agent.tape`

- [ ] **Step 1: Write the hero tape**

```tape
# Clip 1 — Hero: deploy Claude + Codex + Pi to one workspace; they review the
# planted bugs; relay a finding across agents with `wsx agent send`.
Output demo/out/01-hero-raw.mp4
Set Shell "bash"
Set FontSize 16
Set Width 1280
Set Height 720
Set TypingSpeed 55ms
Set Padding 20

Env XDG_STATE_HOME "/tmp/wsx-demo/state"

# Launch wsx; create a workspace on toy-api with claude attached.
Hide
Type "wsx workspace create toy-api --name security-review --yolo --agent claude" Enter
Sleep 2s
Show
Type "wsx" Enter
Sleep 3s

# Attach the other two harnesses to the SAME workspace.
# (Exact keys from SPIKE-NOTES: agents panel is Ctrl-x a; or run from workspace cwd.)
Ctrl+x
Type "a"
Sleep 1s
# add codex, add pi — corrected against spike notes
# ...
Sleep 2s

# Send a narrow review prompt to each agent; Wait blocks on real completion.
Type "Find the most serious security bug in src/auth.py. One sentence."
Enter
Wait+Screen /(SQL inject|auth bypass|injection)/
Sleep 2s

# Cross-agent relay — Pi forwards Claude's finding to Codex for a second opinion.
Type "wsx agent send codex 'Claude flagged SQL injection in auth.py — confirm and suggest a fix.'"
Enter
Wait+Screen /(confirm|parameteri|fix)/
Sleep 3s

Type "q"
Sleep 1s
```

- [ ] **Step 2: Run the tape**

Run: `bash demo/sandbox-bootstrap.sh && vhs demo/tapes/01-hero-multi-agent.tape`
Expected: `demo/out/01-hero-raw.mp4` exists and shows three agents on one workspace with real review output and the cross-agent relay. Re-run if a take is messy (live variance).

- [ ] **Step 3: Sanity-check duration**

Run: `ffprobe -v error -show_entries format=duration -of csv=p=0 demo/out/01-hero-raw.mp4`
Expected: roughly 25–40s. If much longer, tighten prompts/sleeps and re-run.

- [ ] **Step 4: Commit the tape**

```bash
git add demo/tapes/01-hero-multi-agent.tape
git commit -m "demo: hero multi-agent review tape (clip 1)"
```

---

### Task 5: Parallel-worktrees tape (Clip 2)

**Files:**
- Create: `demo/tapes/02-parallel-worktrees.tape`

- [ ] **Step 1: Write the tape**

```tape
# Clip 2 — Parallel worktrees: spin up several workspaces across repos; the
# dashboard shows them running in parallel with live status chips.
Output demo/out/02-parallel-raw.mp4
Set Shell "bash"
Set FontSize 16
Set Width 1280
Set Height 720
Set TypingSpeed 45ms
Set Padding 20

Env XDG_STATE_HOME "/tmp/wsx-demo/state"

Hide
Type "wsx workspace create toy-api --name add-rate-limit --yolo --agent claude" Enter
Sleep 1s
Type "wsx workspace create toy-api --name fix-auth --yolo --agent codex" Enter
Sleep 1s
Type "wsx workspace create toy-cli --name arg-parsing --yolo --agent pi" Enter
Sleep 1s
Show

Type "wsx" Enter
Sleep 3s
# Dashboard now lists three parallel workspaces across two repos.
Screenshot demo/out/02-dashboard.png
Sleep 4s            # let status chips animate to "running"

# Open the updates panel to show parallel status at a glance.
Ctrl+x
Type "u"
Sleep 4s

Type "q"
Sleep 1s
```

- [ ] **Step 2: Run the tape**

Run: `bash demo/sandbox-bootstrap.sh && vhs demo/tapes/02-parallel-worktrees.tape`
Expected: `demo/out/02-parallel-raw.mp4` shows three workspaces across two repos with live status. ~20–30s.

- [ ] **Step 3: Commit**

```bash
git add demo/tapes/02-parallel-worktrees.tape
git commit -m "demo: parallel-worktrees tape (clip 2)"
```

---

### Task 6: Post-production — captions + 10MB budget gate

**Files:**
- Create: `demo/post.sh`
- Create: `demo/captions/01-hero.txt` and `demo/captions/02-parallel.txt`

- [ ] **Step 1: Write a failing check**

Create `demo/test-post.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
# Make a 5s 1280x720 dummy "raw" clip and run post on it.
mkdir -p demo/out
ffmpeg -y -f lavfi -i color=c=black:s=1280x720:d=5 -pix_fmt yuv420p demo/out/dummy-raw.mp4 2>/dev/null
printf '0\t2\tHello caption\n' > /tmp/dummy-caps.txt
"$(dirname "$0")/post.sh" demo/out/dummy-raw.mp4 demo/out/dummy.mp4 /tmp/dummy-caps.txt
test -f demo/out/dummy.mp4 || { echo "FAIL: no output"; exit 1; }
size=$(stat -c%s demo/out/dummy.mp4)
test "$size" -lt 10485760 || { echo "FAIL: over 10MB ($size)"; exit 1; }
echo "PASS"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `bash demo/test-post.sh`
Expected: FAIL — `post.sh: No such file or directory`.

- [ ] **Step 3: Implement `demo/post.sh`**

```bash
#!/usr/bin/env bash
# Caption a raw clip and enforce the 10MB GitHub budget.
# Usage: post.sh <in-raw.mp4> <out.mp4> <captions.tsv>
# captions.tsv lines: <start_s>\t<end_s>\t<text>
set -euo pipefail
IN="${1:?in}"; OUT="${2:?out}"; CAPS="${3:?captions tsv}"
BUDGET=$((9 * 1024 * 1024))   # 9MB target, 1MB headroom under GitHub's 10MB.
FONT="${WSX_DEMO_FONT:-/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf}"

# Build a drawtext filter chain (lower-third box) from the captions TSV.
filter=""
while IFS=$'\t' read -r start end text; do
  [ -z "${start:-}" ] && continue
  esc=$(printf '%s' "$text" | sed "s/'/\\\\'/g; s/:/\\\\:/g")
  filter="${filter}drawtext=fontfile=${FONT}:text='${esc}':fontcolor=white:fontsize=28:box=1:boxcolor=black@0.6:boxborderw=16:x=(w-text_w)/2:y=h-90:enable='between(t,${start},${end})',"
done < "$CAPS"
filter="${filter%,}"
[ -z "$filter" ] && filter="null"

encode() { # <crf> <scale_w> <fps>
  ffmpeg -y -i "$IN" -vf "fps=$3,scale=$2:-2,${filter}" \
    -c:v libx264 -preset slow -crf "$1" -maxrate 3M -bufsize 6M \
    -pix_fmt yuv420p -movflags +faststart -an "$OUT" 2>/dev/null
}

# Step down quality until under budget: (crf, width, fps) ladder.
for cfg in "23 1280 30" "26 1280 24" "28 1120 20" "30 960 18"; do
  read -r crf w fps <<<"$cfg"
  encode "$crf" "$w" "$fps"
  sz=$(stat -c%s "$OUT")
  if [ "$sz" -lt "$BUDGET" ]; then
    echo "OK: $OUT = $((sz/1024))KB (crf=$crf w=$w fps=$fps)"
    exit 0
  fi
  echo "still $((sz/1024))KB > budget; stepping down..."
done
echo "ERROR: could not get $OUT under 9MB — shorten the clip." >&2
exit 1
```

- [ ] **Step 4: Run the check to verify it passes**

Run: `chmod +x demo/post.sh demo/test-post.sh && bash demo/test-post.sh`
Expected: `PASS`. (If the font path differs, set `WSX_DEMO_FONT` to an installed monospace TTF — `fc-list | grep -i mono` to find one.)

- [ ] **Step 5: Write the real caption tracks**

`demo/captions/01-hero.txt`:
```
0	4	One workspace. Three coding agents.
5	11	Claude, Codex, and Pi review in parallel
13	20	SQL injection found in auth.py
22	29	Relay the finding across agents with `wsx agent send`
```
`demo/captions/02-parallel.txt`:
```
0	4	Spin up isolated worktrees in parallel
6	12	Each workspace runs its own agent
14	22	Live status at a glance on the dashboard
```

- [ ] **Step 6: Commit**

```bash
git add demo/post.sh demo/test-post.sh demo/captions
git commit -m "demo: ffmpeg captioning + 10MB budget post pass"
```

---

### Task 7: Makefile + demo README

**Files:**
- Create: `demo/Makefile`
- Create: `demo/README.md`

- [ ] **Step 1: Implement `demo/Makefile`**

```make
SHELL := /usr/bin/env bash
.PHONY: all clips clean hero parallel gif check

all: clips

clean:
	rm -rf $${WSX_DEMO_ROOT:-/tmp/wsx-demo}
	rm -rf out

out:
	mkdir -p out

hero: out
	bash sandbox-bootstrap.sh
	vhs tapes/01-hero-multi-agent.tape
	bash post.sh out/01-hero-raw.mp4 out/01-hero.mp4 captions/01-hero.txt

parallel: out
	bash sandbox-bootstrap.sh
	vhs tapes/02-parallel-worktrees.tape
	bash post.sh out/02-parallel-raw.mp4 out/02-parallel.mp4 captions/02-parallel.txt

clips: hero parallel

# Optional: short GIF loops (auto-sized to stay well under 10MB).
gif: clips
	agg --cols 160 --rows 40 out/01-hero-raw.mp4 out/01-hero.gif 2>/dev/null || \
	  ffmpeg -y -i out/01-hero.mp4 -vf "fps=12,scale=800:-2" out/01-hero.gif

check:
	bash test-gen-repos.sh && bash test-bootstrap.sh && bash test-post.sh
```

Note: `agg` consumes asciinema `.cast` files, not mp4 — if GIFs are wanted, the
simplest path is the ffmpeg `palettegen` route shown in the fallback above. Keep
GIFs short; verify each is < 10MB with `stat -c%s`.

- [ ] **Step 2: Write `demo/README.md`**

Document: prerequisites (`vhs`, `ttyd`, `ffmpeg`, live `claude`/`codex`/`pi`), the
one-command flow (`cd demo && make clips`), where outputs land (`out/`), the 10MB
budget behavior, and that everything is sandboxed under `WSX_DEMO_ROOT` so the
real wsx install is never touched. Reference `SPIKE-NOTES.md` for the driving
method.

- [ ] **Step 3: Run the full check target**

Run: `cd demo && make check`
Expected: all three `test-*.sh` print `PASS`.

- [ ] **Step 4: Commit**

```bash
git add demo/Makefile demo/README.md
git commit -m "demo: Makefile pipeline + demo README"
```

---

### Task 8 (optional / stretch): Diff-review clip (Clip 3)

Only if the author wants a third clip. Mirrors Task 5's structure: a tape that
opens a workspace with real changes and shows `view diff` (custom diff command or
lazygit, per the README "Using external tools" section), then `post.sh` with a
`demo/captions/03-diff.txt` track. Defer until clips 1–2 are approved.

---

### Task 9: Render everything, verify budgets, open PR

- [ ] **Step 1: Full render**

Run: `cd demo && make clean && make clips`
Expected: `demo/out/01-hero.mp4` and `demo/out/02-parallel.mp4` exist.

- [ ] **Step 2: Verify every deliverable is under 10MB**

Run: `cd demo && for f in out/*.mp4 out/*.gif; do [ -e "$f" ] && printf '%s\t%sKB\n' "$f" "$(( $(stat -c%s "$f")/1024 ))"; done`
Expected: every file < 10240KB. If any is over, `post.sh` already errored; shorten that clip's tape.

- [ ] **Step 3: Send finals to the author for review**

Surface `out/01-hero.mp4` and `out/02-parallel.mp4` for a watch-and-notes pass.
Iterate on tapes/captions per feedback (re-run `make hero` / `make parallel`).

- [ ] **Step 4: cargo fmt / clippy sanity (repo hygiene)**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: clean. (No Rust changed, but CI gates on fmt+clippy — see project memory `workspacex-ci-gates`. This guards against accidental churn.)

- [ ] **Step 5: Open the PR**

Use the `pull-request` skill to open a PR from `demo-screencasts` against `main`,
describing the harness, the 10MB-budget design, and embedding the two final clips.

---

## Notes for the implementer

- **Keybindings are first-draft.** `demo/SPIKE-NOTES.md` (Task 1) is the source of
  truth for the exact TUI key sequences; correct every tape against it.
- **Live variance is expected.** Tapes may need 1–2 re-runs for a clean take; this
  is inherent to recording real agents and is acceptable.
- **Never widen scope into the real install.** Every `wsx` call must carry the
  sandbox `XDG_STATE_HOME`. A single un-scoped call could mutate the author's real
  workspaces.
- **GIF is best-effort.** MP4 is the contract (GitHub renders it inline under
  10MB). Only ship a GIF where it fits the budget; otherwise MP4-only.
```
