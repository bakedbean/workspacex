# workspace-x.com — site handoff

Static one-pager. No build step. External requests: two Google Fonts and the
Cloudflare Web Analytics beacon (cookieless) at the end of `index.html`.

## Files

    site/
      index.html      full page markup
      site.css        all styles (design tokens at the top)
      site.js         nav shadow, copy button, scroll reveal, lazy video
      assets/
        01-hero.mp4       screencast 01 — Claude hands a fix to Codex, one workspace
        02-parallel.mp4   screencast 02 — parallel agents across isolated worktrees
        og-cover.png      Open Graph / Twitter card image
        dashboard-hero.{png,webp}  hero still — the dashboard under load
                          (regenerate with `make -C demo hero-still`)
        agent-chat.{png,webp}      still in #see — the attached view, two agents
                          coordinating (`make -C demo agent-chat-still`)

`site.js` sends a HEAD request per `<video data-src>` and only sets `src`
when the file responds 2xx/3xx; a missing file shows the diagonal-hatch
"screencast coming soon" placeholder instead. The slots are 16:9 to match
the current recordings (1280x720 H.264, ~1 MB each). To replace one, keep
the same filename and aspect ratio.

## Deploy

GitHub Pages, via `.github/workflows/docs.yml`: on every push to `main` that
touches `site/**` the workflow copies `site/` to the web root and the mdBook
build of `docs/book` under `/docs/`, which is where the Docs links point.
Nothing is server-rendered. The favicon links use root-absolute paths, so
serving from a subdirectory needs those adjusted; the HEAD probe needs an
HTTP server, so `file://` shows the video placeholders.

## Page structure

1. `.nav` — sticky; gains a bottom border past 8px scroll (`.scrolled`). Below
   600px the Star button hides, below 380px the Quickstart link too, so the
   bar stays on one line down to 320px.
2. `header.hero` — h1, sub line, `$ wsx` prompt with blinking cursor, two hints, CTAs
3. `#features` — value props as `.prop` rows (glyph / name / description)
4. `#how` — quickstart command block with copy button; scrolls sideways
   below 620px instead of truncating commands
5. `#see` — two screencast slots
6. `#cta` — clone line + buttons
7. `footer` — links, tagline, license line

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
- Never set text in `--fg-faint` (2.6:1), footer included. Use `--fg-muted` (5:1) or lighter.
- `.cmd` reserves 56px of right padding for the absolutely-positioned copy button — don't remove it.
- `.reveal` (+ optional `data-d="1..3"` for stagger) fades an element in on scroll; it is reduced-motion safe, has a 2.6s failsafe that force-reveals everything, and a `<noscript>` rule shows everything when JS is off.
- Hero copy states supported harnesses (Claude Code, Codex, oh-my-pi, Hermes) — update there, in the footer tagline and in the JSON-LD when that list changes.
- `<head>` carries canonical, favicons, Open Graph / Twitter cards and JSON-LD; keep them when re-applying a design handoff.

## Accessibility

Dark theme only. All text meets 4.5:1 against its background. Motion is
limited to the cursor blink and reveal fades, both disabled under
`prefers-reduced-motion`. Videos are `controls muted loop playsinline`.
