#!/usr/bin/env python3
"""Turn a record.sh capture dir into a frame list for shoot.mjs.

Usage: timeline.py <rec-dir> [--fps N] [--max-hold S] [--tail S]

Each changed frame plays for as long as it stayed on screen, divided by the
speed in force (`rec_speed`), and capped at --max-hold so an agent booting or
thinking never shows as dead air (`rec_speed real` lifts the cap). Frames that
would last under one output frame merge into the next. Writes
<rec-dir>/frames.js (`window.FRAMES = [{html, caption, dur}, ...]`) and
prints the clip length in seconds.
"""
import json
import os
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "stills"))
from ansi2html import convert  # noqa: E402


def main():
    args = sys.argv[1:]
    rec = args[0]
    opts = dict(zip(args[1::2], args[2::2]))
    fps = float(opts.get("--fps", 30))
    max_hold = float(opts.get("--max-hold", 2.5))
    tail = float(opts.get("--tail", 2.0))
    cols, rows = (int(x) for x in open(os.path.join(rec, "size")).read().split())

    events = []
    for line in open(os.path.join(rec, "index.tsv"), encoding="utf-8"):
        parts = line.rstrip("\n").split("\t", 2)
        if len(parts) == 3:
            events.append((float(parts[0]), parts[1], parts[2]))
    events.sort(key=lambda e: e[0])
    end = next((t for t, k, _ in reversed(events) if k == "end"), events[-1][0])

    caption, speed = "", "1"
    frames = []  # (start, file, caption, speed)
    for t, kind, val in events:
        if kind == "caption":
            caption = val
            # A caption issued mid-frame starts a fresh segment on the same frame.
            if frames:
                frames.append((t, frames[-1][1], caption, speed))
        elif kind == "speed":
            speed = val
            if frames:
                frames.append((t, frames[-1][1], caption, speed))
        elif kind == "frame":
            frames.append((t, val, caption, speed))

    out, acc = [], 0.0
    for i, (t, f, cap, sp) in enumerate(frames):
        nxt = frames[i + 1][0] if i + 1 < len(frames) else end
        d = max(0.0, nxt - t)
        if sp == "real":
            dur = d
        else:
            dur = min(d / float(sp), max_hold)
        acc += dur
        if acc < 1.0 / fps and i + 1 < len(frames):
            continue
        out.append({"file": f, "caption": cap, "dur": round(acc, 3)})
        acc = 0.0
    if out:
        out[-1]["dur"] += tail

    cache = {}
    for fr in out:
        f = fr.pop("file")
        if f not in cache:
            text = open(os.path.join(rec, "frames", f), encoding="utf-8", errors="replace").read()
            lines = text.rstrip("\n").split("\n")
            lines += [""] * max(0, rows - len(lines))
            cache[f] = convert("\n".join(lines[:rows]), cols)
        fr["html"] = cache[f]
    with open(os.path.join(rec, "frames.js"), "w", encoding="utf-8") as fh:
        fh.write("window.FRAMES = ")
        json.dump(out, fh)
        fh.write(";\n")
    print(round(sum(fr["dur"] for fr in out), 1))


if __name__ == "__main__":
    main()
