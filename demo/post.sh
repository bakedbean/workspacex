#!/usr/bin/env bash
# Caption a raw clip and enforce the 10MB GitHub budget.
# Usage: post.sh <in-raw.mp4> <out.mp4> <captions.tsv>
# captions.tsv lines: <start_s>\t<end_s>\t<text>
set -euo pipefail
IN="${1:?in}"; OUT="${2:?out}"; CAPS="${3:?captions tsv}"
BUDGET=$((9 * 1024 * 1024))   # 9MB target, 1MB headroom under GitHub's 10MB.

# Pick an installed monospace font if the default is absent.
# `|| true` guards against SIGPIPE (exit 141) from `head` closing the pipe
# early under `set -o pipefail`.
FONT="${WSX_DEMO_FONT:-}"
if [ -z "$FONT" ] || [ ! -f "$FONT" ]; then
  FONT="$(fc-list : file 2>/dev/null | sed 's/: *$//' | grep -iE 'mono' | head -1 || true)"
fi
[ -n "$FONT" ] && [ -f "$FONT" ] || { echo "ERROR: no monospace font found; set WSX_DEMO_FONT" >&2; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Build a drawtext filter chain (lower-third box) from the captions TSV.
# Each caption's text is written to its own file and referenced via `textfile=`,
# which sidesteps every drawtext text-escaping pitfall (apostrophes, colons,
# em-dashes, %, ...). The commas inside enable='between(t,a,b)' are protected by
# the single quotes.
filter=""
i=0
while IFS=$'\t' read -r start end text; do
  [ -z "${start:-}" ] && continue
  case "$start" in \#*) continue;; esac
  capf="$TMP/cap-$i.txt"
  printf '%s' "$text" > "$capf"
  filter="${filter}drawtext=fontfile='${FONT}':textfile='${capf}':fontcolor=white:fontsize=28:box=1:boxcolor=black@0.6:boxborderw=16:x=(w-text_w)/2:y=h-90:enable='between(t,${start},${end})',"
  i=$((i + 1))
done < "$CAPS"
filter="${filter%,}"
[ -z "$filter" ] && filter="null"

encode() { # <crf> <scale_w> <fps>
  ffmpeg -y -i "$IN" -vf "fps=$3,scale=$2:-2,${filter}" \
    -c:v libx264 -preset slow -crf "$1" -maxrate 3M -bufsize 6M \
    -pix_fmt yuv420p -movflags +faststart -an "$OUT" 2>"$TMP/err"
}

# Step down quality until under budget: (crf, width, fps) ladder.
for cfg in "23 1280 30" "26 1280 24" "28 1120 20" "30 960 18"; do
  read -r crf w fps <<<"$cfg"
  if ! encode "$crf" "$w" "$fps"; then
    echo "ERROR: ffmpeg failed during captioning:" >&2
    tail -4 "$TMP/err" >&2
    exit 1
  fi
  sz=$(stat -c%s "$OUT")
  if [ "$sz" -lt "$BUDGET" ]; then
    echo "OK: $OUT = $((sz/1024))KB (crf=$crf w=$w fps=$fps)"
    exit 0
  fi
  echo "still $((sz/1024))KB > budget; stepping down..."
done
echo "ERROR: could not get $OUT under 9MB — shorten the clip." >&2
exit 1
