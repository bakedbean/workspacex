#!/usr/bin/env bash
# Speed up a single actively-changing span of a clip, leaving the rest at 1x.
# deadair.sh only collapses *static* stretches (via freezedetect); a long span
# where the screen is constantly changing — e.g. an agent churning through
# edit/test/commit tool calls — survives it intact. This ramps just that span.
#
# Usage: speedramp.sh <in.mp4> <out.mp4> <start_s> <end_s> [factor=3.5]
#
# NOTE: start/end are absolute timestamps in <in.mp4> and are TUNED TO THE
# RECORDED TAKE (like the fixed Sleeps in the tapes). A fresh render shifts the
# agent timeline, so re-confirm the span (see demo/SPIKE-NOTES.md) if you
# re-record. The hero uses this to compress Codex's ~35s fix/commit churn while
# keeping the delegate, hand-off, report-back, verify, and outro beats at 1x.
set -euo pipefail
IN="${1:?in}"; OUT="${2:?out}"; START="${3:?start_s}"; END="${4:?end_s}"
FACTOR="${5:-3.5}"

ffmpeg -y -i "$IN" -filter_complex \
"[0:v]trim=0:${START},setpts=PTS-STARTPTS[a];\
[0:v]trim=${START}:${END},setpts=(PTS-STARTPTS)/${FACTOR}[b];\
[0:v]trim=${END},setpts=PTS-STARTPTS[c];\
[a][b][c]concat=n=3:v=1[out]" -map "[out]" \
  -c:v libx264 -preset slow -crf 18 -pix_fmt yuv420p -an "$OUT" 2>/dev/null

indur="$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$IN")"
outdur="$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT")"
printf 'speedramp: %ss -> %ss (%sx over %s-%ss)\n' "$indur" "$outdur" "$FACTOR" "$START" "$END"
