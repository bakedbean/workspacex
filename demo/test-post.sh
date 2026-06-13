#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HERE/out"
# Make a 5s 1280x720 dummy "raw" clip and run post on it.
ffmpeg -y -f lavfi -i color=c=black:s=1280x720:d=5 -pix_fmt yuv420p "$HERE/out/dummy-raw.mp4" 2>/dev/null
printf '0\t2\tHello caption\n' > /tmp/dummy-caps.txt
"$HERE/post.sh" "$HERE/out/dummy-raw.mp4" "$HERE/out/dummy.mp4" /tmp/dummy-caps.txt
test -f "$HERE/out/dummy.mp4" || { echo "FAIL: no output"; exit 1; }
size=$(stat -c%s "$HERE/out/dummy.mp4")
test "$size" -lt 10485760 || { echo "FAIL: over 10MB ($size)"; exit 1; }
rm -f "$HERE/out/dummy-raw.mp4" "$HERE/out/dummy.mp4" /tmp/dummy-caps.txt
echo "PASS"
