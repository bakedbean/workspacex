#!/usr/bin/env bash
# Render a record.sh capture dir to an H.264 MP4.
# Usage: render.sh <rec-dir> <out.mp4>
#   MAX_HOLD   longest any unchanging frame plays, in seconds (default 2.5)
#   SCALE      device pixel ratio for the 1280x720 canvas (default 1.5 -> 1920x1080)
#   CRF        x264 quality (default 20)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REC="${1:?rec dir}"; OUT="${2:?out.mp4}"
# shellcheck source=/dev/null
source "$HERE/../stills/still.sh"   # for _still_chrome
chrome="$(_still_chrome)"

len="$(python3 "$HERE/timeline.py" "$REC" --max-hold "${MAX_HOLD:-2.5}")"
echo "timeline: ${len}s"
n="$(node "$HERE/shoot.mjs" "$REC" "$chrome" "${SCALE:-1.5}")"
echo "shot $n frames"

# Concat demuxer: each PNG with its duration; the last file repeats so its
# duration is honoured.
python3 - "$REC" <<'PY'
import json, os, sys
rec = sys.argv[1]
src = open(os.path.join(rec, "frames.js"), encoding="utf-8").read()
frames = json.loads(src[src.index("=") + 1:].rstrip().rstrip(";"))
with open(os.path.join(rec, "concat.txt"), "w") as fh:
    for i, f in enumerate(frames):
        fh.write(f"file 'png/{i:06d}.png'\nduration {f['dur']:.3f}\n")
    fh.write(f"file 'png/{len(frames) - 1:06d}.png'\n")
PY
mkdir -p "$(dirname "$OUT")"
ffmpeg -y -loglevel error -f concat -safe 0 -i "$REC/concat.txt" \
  -vf "fps=30,format=yuv420p" -c:v libx264 -preset slow -crf "${CRF:-20}" -tune animation \
  -movflags +faststart -an "$OUT"
echo "clip -> $OUT ($(( $(wc -c < "$OUT") / 1024 ))KB)"
