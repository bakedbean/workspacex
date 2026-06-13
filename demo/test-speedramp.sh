#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HERE/out"
# Make a 10s dummy clip whose frames genuinely differ (testsrc has a moving
# pattern + frame counter — no font dependency, so this can't go flaky on a host
# with no monospace font) and ramp the middle 4-8s window 4x. Expect
# ~10 - (4 - 4/4) = ~7s out.
ffmpeg -y -f lavfi -i testsrc=size=320x180:rate=30:duration=10 \
  -pix_fmt yuv420p "$HERE/out/dummy-ramp-in.mp4" 2>/dev/null
"$HERE/speedramp.sh" "$HERE/out/dummy-ramp-in.mp4" "$HERE/out/dummy-ramp-out.mp4" 4 8 4
test -f "$HERE/out/dummy-ramp-out.mp4" || { echo "FAIL: no output"; exit 1; }
dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$HERE/out/dummy-ramp-out.mp4")
# 10s in, middle 4s -> 1s, so ~7s out. Allow a frame of slack either side.
awk -v d="$dur" 'BEGIN{ if (d>6.5 && d<7.5) exit 0; exit 1 }' \
  || { echo "FAIL: expected ~7s, got ${dur}s"; exit 1; }
rm -f "$HERE/out/dummy-ramp-in.mp4" "$HERE/out/dummy-ramp-out.mp4"
echo "PASS"
