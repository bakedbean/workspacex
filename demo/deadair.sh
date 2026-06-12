#!/usr/bin/env bash
# Collapse long static ("dead air") stretches in a screencast: detect frozen
# segments and trim each down to a brief hold, leaving active content at natural
# 1x speed. Reproducible — re-runs freezedetect on whatever clip you give it.
# Usage: deadair.sh <in.mp4> <out.mp4> [min_freeze_s] [max_hold_s]
set -euo pipefail
IN="${1:?in}"; OUT="${2:?out}"
MIN_FREEZE="${3:-5.0}"   # only collapse static stretches longer than this
MAX_HOLD="${4:-1.3}"     # ...down to this much hold

dur="$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$IN")"

# Detect frozen segments (near-identical frames).
freezes="$(ffmpeg -i "$IN" -vf "freezedetect=n=-50dB:d=$MAX_HOLD" -map 0:v -f null - 2>&1 \
  | grep -oE 'freeze_(start|duration): [0-9.]+' | awk '{print $2}')"

# Build the list of time ranges to KEEP, dropping the excess of each long freeze.
keep="$(python3 - "$dur" "$MIN_FREEZE" "$MAX_HOLD" "$freezes" <<'PY'
import sys
dur=float(sys.argv[1]); minf=float(sys.argv[2]); hold=float(sys.argv[3])
vals=[float(x) for x in sys.argv[4].split()]
# freezedetect prints start then duration, paired
pairs=list(zip(vals[0::2], vals[1::2]))
drops=[]  # (a,b) ranges to remove
for start,d in pairs:
    if d > minf:
        drops.append((start+hold, start+d))   # keep first `hold` secs, drop rest
# invert drops -> keep ranges over [0,dur]
keep=[]; cur=0.0
for a,b in sorted(drops):
    a=max(a,cur)
    if a>cur: keep.append((cur,a))
    cur=max(cur,b)
if cur<dur: keep.append((cur,dur))
print(";".join(f"{a:.3f}:{b:.3f}" for a,b in keep))
print("DROPPED %.1fs" % sum(b-a for a,b in drops), file=sys.stderr)
PY
)"

# Build a select expression for the kept ranges and re-time with setpts.
expr=""
IFS=';' read -ra R <<<"$keep"
for r in "${R[@]}"; do
  a="${r%%:*}"; b="${r##*:}"
  [ -n "$expr" ] && expr="${expr}+"
  expr="${expr}between(t,${a},${b})"
done
[ -z "$expr" ] && expr="1"

ffmpeg -y -i "$IN" -vf "select='${expr}',setpts=N/FRAME_RATE/TB" \
  -c:v libx264 -preset slow -crf 20 -pix_fmt yuv420p -an "$OUT" 2>/dev/null
newdur="$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT")"
printf 'deadair: %ss -> %ss\n' "$dur" "$newdur"
