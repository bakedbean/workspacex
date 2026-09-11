#!/usr/bin/env python3
"""Render a `tmux capture-pane -e -p` dump as a self-contained HTML page.

Usage: ansi2html.py <in.txt> <out.html> [--cols N] [--rows N] [--font-size PX]
Prints the CSS-pixel page size (`WxH`) so the caller can size the browser
window. Handles the SGR subset ratatui/tmux emit: reset, bold, dim, italic,
underline, reverse, the 16 named colors, 256-color and 24-bit foreground /
background. Everything else is dropped.
"""
import html
import re
import sys

PAD = 18          # CSS px around the terminal
LINE_HEIGHT = 1.2
CHAR_WIDTH = 0.6  # FiraCode's advance width in em

BG, FG = "#0e1116", "#e8edf4"   # site tokens --bg-1 / --fg
ANSI16 = [
    "#1a232f", "#d36258", "#67c089", "#e4ba6c", "#6ea7d8", "#b78cd0", "#7eb6b0", "#b3bdca",
    "#4a5562", "#e07a70", "#7fd39d", "#f0cc85", "#8dbce6", "#c9a6dd", "#98cbc5", "#e8edf4",
]


def color256(n):
    if n < 16:
        return ANSI16[n]
    if n < 232:
        n -= 16
        r, g, b = n // 36, (n // 6) % 6, n % 6
        lv = [0, 95, 135, 175, 215, 255]
        return "#%02x%02x%02x" % (lv[r], lv[g], lv[b])
    v = 8 + (n - 232) * 10
    return "#%02x%02x%02x" % (v, v, v)


def apply_sgr(params, st):
    p = [int(x) if x else 0 for x in params.split(";")] if params else [0]
    i = 0
    while i < len(p):
        c = p[i]
        if c == 0:
            st.clear()
        elif c == 1:
            st["bold"] = True
        elif c == 2:
            st["dim"] = True
        elif c == 3:
            st["italic"] = True
        elif c == 4:
            st["underline"] = True
        elif c == 7:
            st["reverse"] = True
        elif c == 22:
            st.pop("bold", None); st.pop("dim", None)
        elif c == 23:
            st.pop("italic", None)
        elif c == 24:
            st.pop("underline", None)
        elif c == 27:
            st.pop("reverse", None)
        elif 30 <= c <= 37:
            st["fg"] = ANSI16[c - 30]
        elif 90 <= c <= 97:
            st["fg"] = ANSI16[c - 90 + 8]
        elif 40 <= c <= 47:
            st["bg"] = ANSI16[c - 40]
        elif 100 <= c <= 107:
            st["bg"] = ANSI16[c - 100 + 8]
        elif c == 39:
            st.pop("fg", None)
        elif c == 49:
            st.pop("bg", None)
        elif c in (38, 48) and i + 1 < len(p):
            key = "fg" if c == 38 else "bg"
            if p[i + 1] == 5 and i + 2 < len(p):
                st[key] = color256(p[i + 2]); i += 2
            elif p[i + 1] == 2 and i + 4 < len(p):
                st[key] = "#%02x%02x%02x" % (p[i + 2], p[i + 3], p[i + 4]); i += 4
        i += 1


def style_of(st):
    fg, bg = st.get("fg"), st.get("bg")
    if st.get("reverse"):
        fg, bg = (bg or BG), (fg or FG)
    css = []
    if fg: css.append(f"color:{fg}")
    if bg: css.append(f"background:{bg}")
    if st.get("bold"): css.append("font-weight:700")
    if st.get("dim"): css.append("opacity:.62")
    if st.get("italic"): css.append("font-style:italic")
    if st.get("underline"): css.append("text-decoration:underline")
    return ";".join(css)


CSI = re.compile(r"\x1b\[([0-9;]*)([A-Za-z])")


def convert(text, cols):
    out, st = [], {}
    for line in text.split("\n"):
        pos, width = 0, 0
        for m in CSI.finditer(line):
            chunk = line[pos:m.start()]
            if chunk:
                out.append(f'<span style="{style_of(st)}">{html.escape(chunk)}</span>')
                width += len(chunk)
            if m.group(2) == "m":
                apply_sgr(m.group(1), st)
            pos = m.end()
        chunk = line[pos:]
        if chunk:
            out.append(f'<span style="{style_of(st)}">{html.escape(chunk)}</span>')
            width += len(chunk)
        # Pad to the full width so row backgrounds (selection bars) span the pane.
        if width < cols:
            out.append(f'<span style="{style_of(st)}">{" " * (cols - width)}</span>')
        out.append("\n")
    return "".join(out)


def main():
    args = sys.argv[1:]
    src, dst = args[0], args[1]
    opts = dict(zip(args[2::2], args[3::2]))
    cols = int(opts.get("--cols", 166))
    rows = int(opts.get("--rows", 36))
    fs = float(opts.get("--font-size", 13))
    text = open(src, encoding="utf-8", errors="replace").read().rstrip("\n")
    lines = text.split("\n")
    lines += [""] * max(0, rows - len(lines))
    body = convert("\n".join(lines[:rows]), cols)
    # Estimated size (the caller measures the real one with a --dump-dom pass:
    # the page stamps its rendered <pre> box into data-size on <body>).
    w = round(cols * CHAR_WIDTH * fs + 2 * PAD)
    h = round(rows * LINE_HEIGHT * fs + 2 * PAD)
    page = f"""<!doctype html><meta charset="utf-8"><title>wsx</title><style>
html,body{{margin:0;background:{BG}}}
pre{{display:inline-block;margin:0;padding:{PAD}px;width:max-content;overflow:hidden;vertical-align:top;
background:{BG};color:{FG};font:{fs}px/{LINE_HEIGHT} "FiraCode Nerd Font","JetBrains Mono",ui-monospace,monospace;
font-variant-ligatures:none;white-space:pre}}
</style><pre id="t">{body}</pre>
<script>var r=document.getElementById("t").getBoundingClientRect();
document.body.dataset.size=Math.ceil(r.width)+"x"+Math.ceil(r.height);</script>"""
    open(dst, "w", encoding="utf-8").write(page)
    print(f"{w}x{h}")


if __name__ == "__main__":
    main()
