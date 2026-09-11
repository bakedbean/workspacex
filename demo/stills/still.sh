#!/usr/bin/env bash
# Still-image helpers: drive the sandboxed wsx TUI in a detached tmux server and
# screenshot it through headless Chrome. `source` this after sandbox/env.sh.
#
#   still_up <cols> <rows>      launch the TUI (agent session markers cleared)
#   still_keys <key...>         tmux send-keys (e.g. `still_keys z a`, `still_keys Down`)
#   still_open <repo> <slug>    attach to a workspace by name over the TUI's IPC socket
#   still_shot <out.png>        capture-pane -e -> ansi2html.py -> Chrome PNG (2x)
#   still_down                  kill the tmux server (and every agent under it)
#
# Why not VHS: it needs its own browser+ffmpeg stack and on some hosts fails
# silently; tmux + Chrome are what the e2e harness already leans on, and a text
# capture rendered with the TUI's real colors is pixel-stable across takes.
STILL_SOCK="wsx-still-$$"
STILL_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STILL_COLS=166
STILL_ROWS=36
STILL_FONT_SIZE="${STILL_FONT_SIZE:-13}"

_still_chrome() {
  if [ -n "${CHROME_BIN:-}" ]; then echo "$CHROME_BIN"; return; fi
  for c in "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
           "/Applications/Chromium.app/Contents/MacOS/Chromium" \
           google-chrome chromium chromium-browser; do
    if [ -x "$c" ] || command -v "$c" >/dev/null 2>&1; then echo "$c"; return; fi
  done
  echo "still: no Chrome/Chromium found (set CHROME_BIN)" >&2; return 1
}

still_up() { # <cols> <rows>
  STILL_COLS="${1:-$STILL_COLS}"; STILL_ROWS="${2:-$STILL_ROWS}"
  command -v tmux >/dev/null || { echo "still: tmux not installed" >&2; return 127; }
  # shellcheck source=/dev/null
  source "$STILL_ROOT/sandbox/agent-env.sh"
  local unset_args=(); local v
  for v in "${WSX_AGENT_ENV_UNSET[@]}"; do unset_args+=(-u "$v"); done
  local wsx; wsx="$(command -v "${WSX_BIN:-wsx}")"
  local bindir; bindir="$(cd "$(dirname "$wsx")" && pwd)"
  env "${unset_args[@]}" tmux -L "$STILL_SOCK" new-session -d -x "$STILL_COLS" -y "$STILL_ROWS" \
    -e "XDG_STATE_HOME=$XDG_STATE_HOME" \
    -e "CLAUDE_CONFIG_DIR=$CLAUDE_CONFIG_DIR" \
    -e "CODEX_HOME=$CODEX_HOME" \
    -e "ZDOTDIR=$ZDOTDIR" \
    -e "PATH=$bindir:$PATH" \
    "$wsx"
  # Claude Code nags about focus tracking inside tmux otherwise.
  tmux -L "$STILL_SOCK" set -g focus-events on
  sleep 3   # let the dashboard paint
}

still_keys() { tmux -L "$STILL_SOCK" send-keys "$@"; }

# Attach to <repo>/<slug> the way the desktop integrations do — `select` on
# the TUI's per-process unix socket — so a take never depends on where the
# row happens to sort. Socket location mirrors src/app/ipc.rs::socket_dir.
still_open() { # <repo> <slug>
  local pid dir sock
  pid="$(tmux -L "$STILL_SOCK" list-panes -F '#{pane_pid}' | head -1)"
  if [ -n "${XDG_RUNTIME_DIR:-}" ]; then dir="$XDG_RUNTIME_DIR/wsx"
  elif [ "$(uname)" = Linux ]; then dir="${XDG_STATE_HOME:-$HOME/.local/state}/wsx/run"
  else dir="${TMPDIR:-/tmp}/wsx-run"; fi
  sock="$dir/tui-$pid.sock"
  [ -S "$sock" ] || { echo "still: no TUI socket at $sock" >&2; return 1; }
  python3 - "$sock" "$1" "$2" <<'PYSOCK'
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect(sys.argv[1])
s.sendall(f"select {sys.argv[2]} {sys.argv[3]}\n".encode()); s.close()
PYSOCK
  sleep 2
}

still_shot() { # <out.png>
  local png="$1" txt html size chrome
  txt="${png%.png}.txt"; html="${png%.png}.html"
  tmux -L "$STILL_SOCK" capture-pane -e -p > "$txt"
  size="$(python3 "$STILL_ROOT/demo/stills/ansi2html.py" "$txt" "$html" \
            --cols "$STILL_COLS" --rows "$STILL_ROWS" --font-size "$STILL_FONT_SIZE")"
  chrome="$(_still_chrome)" || return 1
  rm -f "$png"
  # Chrome sometimes never exits after --screenshot; poll for the file instead.
  python3 - "$chrome" "$html" "$png" "$size" "$WSX_SANDBOX_ROOT/chrome-profile" <<'PY' || { echo "still: Chrome produced no screenshot for $png" >&2; return 1; }
import os, re, subprocess, sys, time
chrome, html, png, size, profile = sys.argv[1:]
base = [chrome, "--headless=new", "--disable-gpu", "--no-first-run",
        "--no-default-browser-check", f"--user-data-dir={profile}", "--hide-scrollbars"]
# Measure the rendered <pre> (real font metrics) before sizing the window; fall
# back to the caller's estimate if the dump doesn't carry it.
# (Chrome writes the DOM and then often lingers; give it a moment, then kill and
# read whatever it printed.)
try:
    d = subprocess.Popen(base + ["--dump-dom", "file://" + os.path.abspath(html)],
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    try:
        dom, _ = d.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        d.kill()
        dom, _ = d.communicate()
    m = re.search(r'data-size="(\d+)x(\d+)"', dom or "")
    if m:
        size = f"{m.group(1)}x{m.group(2)}"
except Exception:
    pass
w, h = size.split("x")
p = subprocess.Popen(base + ["--force-device-scale-factor=2", f"--window-size={w},{h}",
    f"--screenshot={os.path.abspath(png)}", "file://" + os.path.abspath(html)],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
deadline, stable, last = time.time() + 60, 0, -1
while time.time() < deadline:
    if p.poll() is not None and os.path.exists(png):
        break
    if os.path.exists(png):
        s = os.path.getsize(png)
        stable = stable + 1 if s == last and s > 0 else 0
        last = s
        if stable >= 4:
            break
    time.sleep(0.25)
if p.poll() is None:
    p.kill()
sys.exit(0 if os.path.exists(png) and os.path.getsize(png) > 0 else 1)
PY
  echo "shot -> $png"
}

still_down() { tmux -L "$STILL_SOCK" kill-server 2>/dev/null || true; }
