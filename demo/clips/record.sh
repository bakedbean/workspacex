#!/usr/bin/env bash
# Screencast recorder for the tmux-driven TUI. `source` after demo/stills/still.sh
# (it records whatever pane `still_up` started).
#
#   rec_start <dir>          start sampling the pane into <dir>/frames (changed frames only)
#   rec_caption <text>       caption from now until the next caption ("" clears it)
#   rec_speed <n|real>       play what follows n times faster; `real` = 1x with no hold cap
#   rec_type <text>          type <text> a key at a time, like a person
#   rec_wait <regex> <secs>  block until the pane matches <regex> (extended grep)
#   rec_stop                 stop sampling
#
# demo/clips/render.sh turns <dir> into an MP4. Marks are timestamped with
# $EPOCHREALTIME, so captions and speed changes land on the frames they were
# issued beside, however long the agents take.
REC_INTERVAL="${REC_INTERVAL:-0.12}"

rec_mark() { printf '%s\t%s\t%s\n' "$EPOCHREALTIME" "$1" "${2:-}" >> "$REC_DIR/index.tsv"; }

rec_start() { # <dir>
  REC_DIR="$1"
  rm -rf "$REC_DIR"; mkdir -p "$REC_DIR/frames"; : > "$REC_DIR/index.tsv"
  printf '%s\t%s\n' "$STILL_COLS" "$STILL_ROWS" > "$REC_DIR/size"
  touch "$REC_DIR/.on"
  (
    n=0 prev=""
    while [ -e "$REC_DIR/.on" ]; do
      t="$EPOCHREALTIME" f="$REC_DIR/frames/$(printf %06d "$n").txt"
      tmux -L "$STILL_SOCK" capture-pane -e -p > "$f.tmp" 2>/dev/null || break
      if [ -n "$prev" ] && cmp -s "$f.tmp" "$prev"; then
        rm -f "$f.tmp"
      else
        mv "$f.tmp" "$f"; printf '%s\tframe\t%s\n' "$t" "${f##*/}" >> "$REC_DIR/index.tsv"
        prev="$f"; n=$((n + 1))
      fi
      sleep "$REC_INTERVAL"
    done
  ) &
  REC_PID=$!
}

rec_caption() { rec_mark caption "$1"; }
rec_speed() { rec_mark speed "$1"; }

rec_type() { # <text>
  local s="$1" i
  for ((i = 0; i < ${#s}; i++)); do
    tmux -L "$STILL_SOCK" send-keys -l -- "${s:i:1}"; sleep 0.045
  done
}

rec_wait() { # <regex> <timeout-s>
  local t=0
  until tmux -L "$STILL_SOCK" capture-pane -p | grep -Eq -- "$1"; do
    [ "$t" -ge "$(( ${2:-120} * 2 ))" ] && { echo "rec: timeout waiting for /$1/" >&2; return 1; }
    sleep 0.5; t=$((t + 1))
  done
}

rec_stop() {
  rm -f "$REC_DIR/.on"; wait "$REC_PID" 2>/dev/null || true
  rec_mark end
}
