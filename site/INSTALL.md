# workspacex site — install instructions

## Files

- `index.html` — the one-pager
- `workspacex-site.css` — marketing styles
- `workspacex-site.js` — interactions (nav, copy buttons, scroll reveal, hero typing)
- `tui.css` — shared TUI palette (used by the hero dashboard demo)

## Install

Copy everything in this folder into your `site/` directory:

```bash
cp -r site_package/* /path/to/workspacex/site/
```

## Screencasts

Create an `assets/` subfolder next to `index.html` and drop in:

```
assets/
  01-hero.mp4
  02-parallel.mp4
```

The video elements will detect the files automatically — no code changes needed.

## Fonts

Loaded from Google Fonts (CDN). Requires internet access:
- JetBrains Mono (mono headlines, code blocks, TUI demo)
- IBM Plex Sans (body, hero headline)

For offline/self-hosted use, download both families from fonts.google.com and update the `<link>` in `index.html` to point at local files.
