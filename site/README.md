# workspace-x.com — site handoff

Static one-pager. No build step, no dependencies beyond two Google Fonts.

## Files

    site/
      index.html      full page markup
      site.css        all styles (design tokens at the top)
      site.js         nav shadow, copy button, scroll reveal, lazy video
      assets/
        01-hero.mp4       screencast 01 — dashboard triage  (ADD THIS)
        02-parallel.mp4   screencast 02 — review / remote    (ADD THIS)

Drop the two mp4s into `assets/` and they appear automatically: `site.js`
sends a HEAD request per `<video data-src>` and only sets `src` when the file
responds 2xx/3xx. Until then the diagonal-hatch "screencast coming soon"
placeholder shows and the console stays clean. Recommended: H.264 mp4, 16:10,
muted, ~1600px wide, under ~8 MB each.

## Deploy

Any static host. Copy `site/` to the web root — e.g. GitHub Pages from
`/site`, or `netlify deploy --dir=site`. Nothing is server-rendered and there
are no absolute paths, so it also works from a subdirectory or `file://`.

## Page structure

1. `.nav` — sticky; gains a bottom border past 8px scroll (`.scrolled`)
2. `header.hero` — h1, sub line, `$ wsx` prompt with blinking cursor, two hints, CTAs
3. `#features` — value props as `.prop` rows (glyph / name / description)
4. `#how` — quickstart command block with copy button
5. `#see` — two screencast slots
6. `#cta` — clone line + buttons
7. `footer`

## Design system

Tokens live in `:root` at the top of `site.css`. The palette and status colors
mirror the product's own TUI theme.

| token | value | use |
| --- | --- | --- |
| `--bg` / `--bg-1` / `--bg-2` / `--bg-3` | #0a0d12 / #0e1116 / #131a23 / #1a232f | page, surface, elevated, hover |
| `--rule` / `--rule-soft` | #1e2733 / #18202b | borders, hairlines |
| `--fg` / `--fg-dim` / `--fg-muted` | #e8edf4 / #b3bdca / #76828f | body text scale |
| `--fg-faint` | #4a5562 | borders and glyph strokes only — fails contrast for text |
| `--accent` | #5ab0ff | prompts, links, primary button |
| `--c-question` … `--c-idle` | amber / red / blue / violet / green / grey | status glyphs on `.prop` rows |

Type: JetBrains Mono for everything structural (headline, prompts, section
heads, prop names); IBM Plex Sans for prose. Nothing is larger than 21px —
the page is deliberately small-type and dense.

## Conventions worth keeping

- Section heads read as commands: `$ wsx --features`. Keep that pattern if you add sections.
- Never set body text in `--fg-faint` (2.6:1). Use `--fg-muted` (5:1) or lighter.
- `.cmd` reserves 56px of right padding for the absolutely-positioned copy button — don't remove it.
- `.reveal` (+ optional `data-d="1..3"` for stagger) fades an element in on scroll; it is reduced-motion safe and has a 2.6s failsafe that force-reveals everything.
- Hero copy states supported harnesses (Claude Code, Codex, oh-my-pi, Hermes) — update there when that list changes.

## Accessibility

Dark theme only. All text meets 4.5:1 against its background. Motion is
limited to the cursor blink and reveal fades, both disabled under
`prefers-reduced-motion`. Videos are `controls muted loop playsinline`.
