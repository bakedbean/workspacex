# Chat-view model + token usage in the bottom line

**Date:** 2026-07-02
**Status:** Approved design, pending implementation plan

## Problem

The attached agent chat view has a bottom "chip row" that already surfaces
live workspace signals in a right-justified block — the running-process count
(`● Np`), the diff count (`+A −R`), and the PR chip (`⏺ #152 open`). The
dashboard's SESSION SUMMARY detail module additionally surfaces the session's
**token usage** (context-window fill), but that signal is absent from the chat
view. The **current model** is not shown anywhere in the UI today — `model_id`
is read from `WorkspaceEvents` but used only internally to resolve the context
window size.

We want both the current model and the token usage visible on the chat view's
bottom line, right-justified alongside the existing procs / diff / PR elements.

## Goal

Add a single combined element — e.g. `opus 4.8 45k/200k` — to the flush-right
block in `render_chip_row`, sourced from data already in scope at the render
call site, formatted consistently with the dashboard detail bar.

## Non-goals

- No changes to how model/token data is collected (`WorkspaceEvents` already
  carries `model_id` and `context_tokens`, populated in `app/background.rs`).
- No changes to the dashboard detail bar itself.
- No new display in the project-manager (PM) pane — it has no workspace/agent
  events.
- No graceful "shrink" of the element under width pressure; it drops whole.

## Current state (reference)

- `render_chip_row` — `src/ui/attached/chip_row.rs`. Builds a `Vec` of optional
  `(spans, width)` elements (procs, diff, PR), joined by single spaces, painted
  flush-right. When the row is too narrow for the whole block plus a 2-cell rule
  gap, elements are dropped **from the left**; the PR chip (rightmost) is kept
  longest. `procs == 0` and a clean/absent diff each render nothing.
- Call site — `src/app/render.rs` (focused-attached branch, ~line 438–458).
  `pr`, `diff`, `procs` are derived for `focused_id`; `app.workspace_events`
  `.get(&focused_id)` in the same scope carries `model_id` + `context_tokens`.
- `render_panes` — `src/ui/attached/mod.rs:77` — passes procs/diff/pr through to
  `render_chip_row` (`mod.rs:148`).
- Other `render_chip_row` / `render_panes` callers: the `AttachedPm` branch
  (`render.rs:642`) and `dashboard/detail.rs:168`, both of which pass zero/None
  for the live signals.
- Formatting helpers already exist in `src/detail_modules/session_summary.rs`:
  - `abbreviate_tokens(n)` → `950` / `77k` / `1M` / `1.2M`.
  - `resolve_window(context_tokens, model_id)` → `Some(200_000)` /
    `Some(1_000_000)` for known Claude families (upgrades past 200k), else
    `None`.
  - `format_context_line(evt)` → the detail bar's
    `context: 45k / 200k · 22%` string + warn flag (warn at ≥ 85%, or raw
    tokens ≥ 150k when the window is unknown).

## Design

### Display element

A new combined element joins the flush-right block, positioned **leftmost**
(lowest priority — dropped first when the row narrows). Block order left→right
becomes: **model+tokens → procs → diff → PR chip**, each separated by one space.

```
opus 4.8 45k/200k  ● 3p  +12 −3  ⏺ #152 open
```

### Text format

`{model label} {tokens}`:

- **Model label** — new `short_model_label(model_id: &str) -> String`:
  - Parses Claude family + version: `claude-opus-4-8[1m]` → `opus 4.8`,
    `claude-sonnet-5` → `sonnet 5`, `claude-haiku-4-5-20251001` → `haiku 4.5`.
    A trailing `[1m]` suffix and a trailing date segment are stripped; the
    version is the first one or two numeric segments after the family, rendered
    dot-joined (`4-8` → `4.8`, `5` → `5`).
  - Non-Claude or unparseable ids fall back to the raw id truncated to ~12
    chars.
- **Token portion** — reuses `abbreviate_tokens` + `resolve_window`:
  - Window resolvable → `{used}/{window}` with no spaces, e.g. `45k/200k`
    (note: compact, unlike the detail bar's `45k / 200k`).
  - Window unknown → raw `{used}`, e.g. `45k`.

### Visibility & color

- Renders only when `context_tokens > 0` (0 / absent → element omitted, matching
  how `procs == 0` and a clean diff collapse). If `model_id` is absent but tokens
  are present, only the token portion shows.
- **Warn color** (`theme.warn_style()`) when fill ≥ 85% of a resolvable window,
  or raw tokens ≥ 150k when the window is unknown — identical thresholds to the
  detail bar. Otherwise `theme.dim_style()`. The whole element takes one style.
- Not clickable (like procs and diff; only the PR chip is a hit target).

### Drop behavior

The combined element is a single `(spans, width)` entry, leftmost in the block.
The existing "drop from the left until it fits" loop in `render_chip_row` handles
it with no new logic: under width pressure the model+tokens element drops first
(whole), then procs, then diff, keeping the PR chip visible longest.

### Plumbing

1. **New formatter** in `session_summary.rs`:
   ```rust
   pub(crate) fn format_chip_model_tokens(evt: &WorkspaceEvents)
       -> Option<(String, bool)>
   ```
   Returns `(text, warn)` per the rules above, or `None` when there's no token
   data. Reuses `abbreviate_tokens` / `resolve_window`; calls the new
   `short_model_label`.
2. **Call site** — `src/app/render.rs`, focused-attached branch, next to the
   existing procs/diff/pr derivations:
   ```rust
   let model_tokens = app
       .workspace_events
       .get(&focused_id)
       .and_then(session_summary::format_chip_model_tokens);
   ```
3. **Thread through** `render_panes` (`src/ui/attached/mod.rs`) and
   `render_chip_row` (`src/ui/attached/chip_row.rs`) as a new
   `model_tokens: Option<(String, bool)>` parameter.
4. In `render_chip_row`, build the element from `model_tokens` (warn flag →
   style) and push it **first** into the `elements` vec (before procs).
5. **Other callers pass `None`:** `AttachedPm` (`render.rs:642`) and
   `dashboard/detail.rs:168`.

`short_model_label` and `format_chip_model_tokens` live in `session_summary.rs`
alongside the existing token helpers so the chip row and the detail bar stay in
lockstep (mirroring the existing "matches the dashboard" pattern in the chip-row
comments).

## Testing

**Unit — `short_model_label`:**
- Versioned Claude ids → `opus 4.8`, `sonnet 5`, `haiku 4.5`.
- `[1m]` suffix stripped (`claude-opus-4-8[1m]` → `opus 4.8`).
- Trailing date segment ignored (`claude-haiku-4-5-20251001` → `haiku 4.5`).
- Unknown/non-Claude id → truncated fallback, no panic.

**Unit — `format_chip_model_tokens`:**
- Known window → `opus 4.8 45k/200k`, warn `false`; ≥ 85% → warn `true`.
- Unknown window → `{label} 45k`, warn at ≥ 150k.
- `context_tokens` `None` / `0` → returns `None`.
- `model_id` absent but tokens present → token-only string.

**Render — chip row (`TestBackend`, following existing chip_row tests):**
- Element paints leftmost in the block, one space before procs, flush-right
  block unchanged for procs/diff/PR.
- Element drops first when the row is too narrow, PR chip stays flush-right.
- Warn styling applies to the element when the warn flag is set.
- Omitted entirely when `model_tokens` is `None`.

## Files touched

- `src/detail_modules/session_summary.rs` — add `short_model_label`,
  `format_chip_model_tokens`, tests.
- `src/ui/attached/chip_row.rs` — new param, element construction, render tests.
- `src/ui/attached/mod.rs` — thread param through `render_panes`.
- `src/app/render.rs` — derive `model_tokens`, pass to both `render_panes`
  calls (`None` for PM).
- `src/ui/dashboard/detail.rs` — pass `None` at the `render_chip_row` call.
