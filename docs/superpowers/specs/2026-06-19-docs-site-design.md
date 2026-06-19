# Documentation site for wsx (mdBook)

**Date:** 2026-06-19
**Status:** Approved design — ready for implementation planning

## Problem

`README.md` is the only documentation and has grown to ~1350 lines / 92 KB. It is
hard to navigate, has no search, and its hand-maintained table of contents is the
only way to jump around. We want a proper, navigable, searchable documentation
site, with the README reduced to a landing page.

## Goals

- Navigable, searchable docs hosted as a static site.
- The README becomes a slim landing page that links to the site.
- The book becomes the single source of truth for detailed docs (no content
  duplicated between README and book — avoids drift).
- Auto-published on every push to `main`.
- Rust-only toolchain (no Node/Python build dependency).

## Non-goals

- No information-architecture overhaul. Content migrates near-verbatim; section
  structure is preserved (1:1 mapping from current README headings).
- No branded marketing landing page, custom domain, or versioned docs (can come
  later).
- No changes to `demo/`, `skills/`, `src/`, or any product code.

## Decisions (from brainstorming)

| Decision | Choice |
|----------|--------|
| Primary goal | Navigable & searchable; README slimmed to landing page |
| Generator | **mdBook** (Rust-native, single binary, built-in search) |
| Deploy | **Auto-deploy via GitHub Actions** to GitHub Pages |
| Content migration | **Split 1:1**, README slimmed, book is single source of truth |
| Page granularity | Each H3 becomes its own nested page (better deep-linking + search) |

## Architecture

### Layout

mdBook project rooted at `docs/book/`:

```
docs/book/
  book.toml            # mdBook config
  src/
    SUMMARY.md         # sidebar / nav (replaces README TOC)
    introduction.md    # book landing page
    overview/...
    daily-use/...
    configuration/...
    integrations/...
    cli-reference/...
    reference/...
    development/...
  book/                # build output — gitignored
```

`docs/` already contains `manual-tests/` and `superpowers/`; the book sits beside
them under `docs/book/` so all docs stay under `docs/`.

### book.toml

```toml
[book]
title    = "wsx (WorkspaceX)"
authors  = ["Eben Goodman"]
language = "en"
src      = "src"

[output.html]
git-repository-url = "https://github.com/bakedbean/workspacex"
edit-url-template  = "https://github.com/bakedbean/workspacex/edit/main/docs/book/{path}"
site-url           = "/workspacex/"   # GitHub Pages project path
default-theme      = "navy"

[output.html.search]
enable = true
```

`site-url = "/workspacex/"` is required so internal links and assets resolve under
the project-pages base path `https://bakedbean.github.io/workspacex/`.

### Build output

`docs/book/book/` is the build artifact. Add it to `.gitignore`. (The doubled
`book/book/` path is mdBook's default; acceptable and conventional.)

## Content migration (1:1)

Each current README H2 becomes a chapter; each H3 becomes its own nested page.
~30 pages total. Content is moved near-verbatim — see "Mechanical edits" below for
the only changes.

SUMMARY.md structure:

```
# Summary

[Introduction](introduction.md)

- [Overview](overview/index.md)
  - [Key features](overview/key-features.md)
  - [Quick start](overview/quick-start.md)
  - [Next steps: wiring up your tools](overview/wiring-up-tools.md)
- [Daily use](daily-use/index.md)
  - [Keybindings](daily-use/keybindings.md)
  - [Pinned commands](daily-use/pinned-commands.md)
  - [Mouse, scrollback, and text selection](daily-use/mouse-scrollback-selection.md)
  - [Dashboard status indicators](daily-use/status-indicators.md)
  - [Process tracking](daily-use/process-tracking.md)
  - [Workspace detail bar](daily-use/detail-bar.md)
  - [Workspace updates panel](daily-use/updates-panel.md)
  - [Split panes](daily-use/split-panes.md)
  - [Project manager pane](daily-use/project-manager-pane.md)
- [Configuration and customization](configuration/index.md)
  - [Global settings](configuration/global-settings.md)
  - [Themes](configuration/themes.md)
  - [Auto-rename modes](configuration/auto-rename-modes.md)
  - [Change chronology](configuration/change-chronology.md)
  - [Coding agents](configuration/coding-agents.md)
  - [Multi-agent workspaces](configuration/multi-agent-workspaces.md)
  - [Per-repo setup scripts](configuration/per-repo-setup-scripts.md)
- [Integrations and remote access](integrations/index.md)
  - [Editor, terminal, and diff integration](integrations/editor-terminal-diff.md)
  - [Remote access](integrations/remote-access.md)
  - [Remote control](integrations/remote-control.md)
  - [Named remote shortcuts](integrations/named-remote-shortcuts.md)
  - [MCP server inheritance](integrations/mcp-inheritance.md)
  - [Related repos](integrations/related-repos.md)
  - [Agent skill](integrations/agent-skill.md)
- [CLI reference](cli-reference/index.md)
  - [Launch the TUI](cli-reference/launch-tui.md)
  - [Repository management](cli-reference/repository-management.md)
  - [Workspace management](cli-reference/workspace-management.md)
  - [Commands documented elsewhere](cli-reference/documented-elsewhere.md)
- [Reference](reference/index.md)
  - [Environment variables](reference/environment-variables.md)
  - [Storage and configuration files](reference/storage-and-config-files.md)
- [Development](development/index.md)
  - [Testing](development/testing.md)
```

Each chapter `index.md` holds the prose that currently sits directly under the H2
(before its first H3); if there is none, it is a short intro sentence linking to
its pages.

### Mechanical edits during migration

These are the *only* content changes; prose is otherwise verbatim:

1. **Cross-links rewritten.** README intra-doc anchor links (`[…](#some-section)`)
   become relative book page links (e.g. `[…](../configuration/themes.md)`).
2. **TOC dropped.** The hand-maintained Table of contents (README lines ~13–54) is
   removed; `SUMMARY.md` replaces it.
3. **Heading levels.** The H2/H3 that became a page title is removed from the page
   body (the filename/SUMMARY entry is the title); deeper headings shift up one
   level as needed so each page starts at H1.

### introduction.md

The book's landing page: one-paragraph "what is wsx", key-features summary, and
links into Overview → Quick start. The demo videos are *not* embedded here (they
are GitHub user-attachment URLs that only auto-embed on github.com); instead link
to the README/repo for the demos.

## Slimmed README (~80 lines)

`README.md` is reduced to:

- Title + tagline.
- The two demo videos (kept here — they embed on github.com).
- A short "Key features" blurb (a few bullets).
- Install + Quick start (the minimal commands to get running).
- A prominent **"📖 Full documentation → https://bakedbean.github.io/workspacex/"**
  link.
- License and a one-line pointer to Development docs.

All other content lives only in the book.

## Deployment

`.github/workflows/docs.yml`:

- Trigger: `push` to `main`, path-filtered to `docs/book/**` and `README.md`.
  Also `workflow_dispatch` for manual runs.
- Steps: checkout → install pinned mdBook (e.g. via released binary, version
  pinned for reproducibility) → `mdbook build docs/book` → upload artifact →
  `actions/deploy-pages`.
- Permissions: `pages: write`, `id-token: write`. Concurrency group so overlapping
  pushes don't race.

**One-time manual prerequisite (cannot be automated here):** in repo Settings →
Pages, set Source = "GitHub Actions". This will be called out in the PR
description.

## Verification

- `mdbook build docs/book` completes with no errors or warnings.
- `mdbook serve docs/book` renders locally; sidebar nav matches SUMMARY; search box
  returns results for sample queries (e.g. "keybindings", "theme").
- No stale anchor links: grep the migrated `src/` for `](#` to confirm all
  intra-doc anchors were rewritten to page links.
- No content left only in the README that was meant to move (spot-check each
  chapter against the original section).
- CI workflow builds successfully on the PR branch.

## Commit plan (feature branch → PR)

1. Scaffold: `book.toml`, `src/SUMMARY.md` (with placeholder pages), `.gitignore`
   entry for build output.
2. Migrate content into pages (the bulk; mechanical edits applied).
3. Slim the README.
4. Add `.github/workflows/docs.yml`.

Per repo policy: all work on a feature branch, opened as a PR against `main`; never
pushed directly to `main`.

## Risks / open considerations

- **Link drift:** rewriting ~README cross-links by hand risks misses — mitigated by
  the `](#` grep check in verification. Optional future add: `mdbook-linkcheck`.
- **Pages base path:** if `site-url` is wrong, assets/links 404 under the project
  path — verified by loading the deployed site once after first publish.
- **mdBook version:** pin in CI for reproducible builds; document the version.
