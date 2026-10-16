#!/usr/bin/env bash
# timeline.py: hold cap, speed marks, caption segments, sub-frame merging.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/frames"
printf '20\t2\n' > "$T/size"
for i in 0 1 2 3; do printf 'frame %s\n' "$i" > "$T/frames/00000$i.txt"; done
cat > "$T/index.tsv" <<'TSV'
100.0	caption	hello
100.0	frame	000000.txt
110.0	frame	000001.txt
110.01	frame	000002.txt
111.0	speed	4
115.0	speed	1
115.0	caption	
115.0	frame	000003.txt
116.0	end	
TSV
len="$(python3 "$HERE/timeline.py" "$T" --max-hold 2.5 --tail 0)"
python3 - "$T/frames.js" "$len" <<'PY'
import json, sys
src = open(sys.argv[1]).read()
f = json.loads(src[src.index("=") + 1:].rstrip().rstrip(";"))
durs = [x["dur"] for x in f]
caps = [x["caption"] for x in f]
# 10s idle capped to 2.5; the 10ms frame merges into the next; 1s at 1x;
# 4s at 4x = 1s; last 1s.
assert durs == [2.5, 1.0, 1.0, 1.0], durs
assert caps == ["hello", "hello", "hello", ""], caps
assert "frame 3" in f[-1]["html"], f[-1]["html"]
assert float(sys.argv[2]) == 5.5, sys.argv[2]
PY
echo "test-timeline: ok"
