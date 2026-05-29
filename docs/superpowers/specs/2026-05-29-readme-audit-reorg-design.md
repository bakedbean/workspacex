# README audit & reorganization — design

**Date:** 2026-05-29

## Problem

`README.md` has grown to 936 lines across 22 top-level (`##`) sections with
no table of contents and no higher-order grouping. Symptoms of append-only
growth: each feature was documented as it shipped and tacked on, rather than
slotted into a logical home.

Three concrete problems:

1. **Related topics scattered.** Remote functionality is split across three
   distant sections — `Named remote shortcuts` (under CLI reference),
   `Remote access` (tmux+ssh), `Remote control` (claude.ai/mobile). TUI
   features (`Process tracking`, `Dashboard status indicators`, `Workspace
   detail bar`, `Workspace updates panel`, `Split panes`, `Project manager
   pane`) are interleaved with config topics. Customization (`Global
   settings`, `Themes`, `Auto-rename modes`, `Coding agents`, `Per-repo setup
   scripts`, `MCP server inheritance`) is spread throughout.
2. **No grouping into parts.** All 22 sections sit at one `##` level, so a
   reader scrolling can't tell which zone they're in.
3. **Reading order has no arc.** Reference tables sit in the middle of
   feature explanations.

## Decisions

- **Scope:** restructure (reorder, regroup, add TOC, consolidate scattered
  topics) **and tighten prose** (dedup redundant passages). Single file —
  no split into `docs/`.
- **Reading arc:** TUI-first. Lead with the interactive experience; push the
  full CLI command reference toward the back as a lookup section. Matches the
  tool's positioning ("Terminal UI for...").

## Target structure

Seven `##` parts; related sections become `###`; deep-dives become `####`.
A two-level table of contents goes right after the intro line.

```
# wsx
## Table of contents                     (NEW)
## Overview
   ### Key features
   ### Quick start
## Daily use — dashboard & sessions
   ### Keybindings                       (merges Dashboard + Modals + Attached)
       #### Pinned commands
       #### Mouse, scrollback, and text selection
   ### Dashboard status indicators
       #### Activity sub-line / Diff counts column / Attention alerts
   ### Process tracking
   ### Workspace detail bar
       #### Schema and defaults / Setting the global value / Per-repo override / Behavior on bad input
   ### Workspace updates panel
   ### Split panes
   ### Project manager pane
## Configuration & customization
   ### Settings & the config CLI         (was "Global settings")
   ### Themes
   ### Auto-rename modes
   ### Coding agents
       #### Hermes integration
   ### Per-repo setup scripts
       #### Editing repo settings in the TUI   (was "Editing in the TUI")
## Integrations & remote access
   ### Editor, terminal, and diff integration
       #### {path} placeholder / Diff command
   ### Remote access (tmux + ssh)
   ### Remote control (claude.ai / mobile)
   ### Named remote shortcuts
   ### MCP server inheritance
   ### Related repos
   ### Claude Code skill
## CLI reference
   ### Launch the TUI / Repository management / Workspace management
   (config / remote / skill commands → one-line pointers to their home sections)
## Reference
   ### Environment variables
   ### Storage and configuration files
## Development
   ### Testing
```

## Prose-tightening targets

No information loss — dedup only:

1. `set-name` is documented twice in Repository management — collapse to one.
2. The three-dot diff rationale appears twice (inline comment + "Why three
   dots?" block) — keep one clear explanation.
3. The `@file` / `""`-clears / value-source convention is re-explained in
   ~5 places — state it once in the config section, cross-reference elsewhere.
4. Config-key prose that duplicates the settings table verbatim — trim the
   section intro so the table is the quick-ref and the section adds only detail.

## Constraints

- No change to factual content, command syntax, or examples.
- **Anchor safety:** demoting `##`→`###` does not change a heading's anchor.
  Only two headings are renamed (`Global settings`→`Settings & the config
  CLI`, `Editing in the TUI`→`Editing repo settings in the TUI`); every
  intra-doc link to those must be updated.

## Delivery

Two logical commits, each independently reviewable:

1. Structural reorg + TOC + anchor fixes (content moved verbatim).
2. Prose tightening (the four dedup targets above).
