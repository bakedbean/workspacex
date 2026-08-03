# Dashboard condensed-recap column

**Date:** 2026-08-03
**Status:** Approved design, pending implementation

## Problem

The dashboard's largest column (the flex column in each workspace row) currently shows
either a pushed status message or a derived "last agent message". Both waste the space:
a bare status fills almost nothing, and a raw agent message is too long to scan and
rarely establishes context on its own. The Project Manager pane already solves the
context problem with agent-authored goal/state/next recaps; the dashboard should show a
distilled, one-line version of the same information.

## Solution overview

The workspace agent authors **short forms** of the three recap fields — keyword
distillations, not truncations (e.g. goal "Audit all V2 invoices auto-issued today for
the CV-04964 amount-drift bug fixed in PR #2835" → goal-short "Audit V2 invoices,
CV-04964, bug from #2835"). The dashboard composes the flex column as:

```
<status token> · <goal-short> · <state-short> · <next-short>
```

Examples at different widths:

```
│ └ working · audit V2 invoices #2835 · 3/12 done · fix drift calc │
│ └ asking · audit V2 invoices #2835 · 3/12 done                   │  (narrower: next dropped)
│ └ stalled · audit V2 invoices #28…                               │  (narrowest: goal truncates)
```

The status token is always present, so the status signal is never lost; the recap
segments fill the remaining width.

## 1. Schema & store

- New migration block (`if v < 20`; V18/V19 were already taken by scm_cache) adds three nullable columns to `workspace_recap`:
  `goal_short TEXT`, `state_short TEXT`, `next_short TEXT`.
- Because wsx re-runs every migration block on each launch, each `ALTER TABLE ... ADD
  COLUMN` must be guarded by a column-existence check (`pragma_table_info`), not just a
  version comparison.
- `WorkspaceRecap` (src/data/store.rs) gains `goal_short`, `state_short`, `next_short`
  as `Option<String>`.
- `set_workspace_recap` extends the existing `COALESCE(excluded.x, workspace_recap.x)`
  partial upsert to the new columns: setting `--state-short` alone updates only that
  field and bumps `updated_at`. `workspace_recap` / `all_workspace_recaps` read the new
  columns; `clear_workspace_recap` is unchanged (row delete).

## 2. CLI

- `wsx recap set` gains `--goal-short`, `--state-short`, `--next-short`. The
  at-least-one-flag rule counts all six flags.
- `wsx recap show` prints the short forms alongside the full fields.
- Help text documents the distillation convention with the worked example above so
  agents reading `--help` produce the right style.

## 3. Dashboard column composition

A new composer in `src/ui/dashboard/column_content.rs` replaces the current
precedence chain for the flex column text.

### Status token

- If a fresh agent-pushed status exists (same freshness gate as today:
  `reported_at >= last_log_activity_ms`, `Busy` exempt), the token is its state word:
  `working` / `waiting` / `blocked` / `done`.
- Otherwise the token is the derived `Status` label: `asking`, `stalled`, `thinking`,
  `waiting`, `done`, `idle`.
- The token keeps status coloring; `Question` and `Stalled` keep their warn emphasis so
  attention states still pop visually. The 2-char status glyph column is unchanged.

### Recap segments

- Segments in order: goal-short, state-short, next-short, joined with ` · `.
- **Greedy width fitting:** append segments while the composed line fits the column
  width; when one doesn't fit, drop it and everything after it. The last included
  segment truncates with `…` only if even the first segment (goal) can't fit whole.
  Net effect as width shrinks: lose `next`, then `state`, then `goal` truncates.
- **Per-field fallback:** a missing short form falls back to its full field clipped to
  32 chars. A field missing entirely (neither form) is skipped, not rendered as a
  placeholder.
- **No recap at all:** the line is `token · <today's column text>` — the existing
  derived text (last agent message, first user text, "asking: …", etc.). Nothing
  regresses for workspaces that predate the convention.
- **Staleness:** reuse the PM pane rule (`last_activity_ms > updated_at +
  RECAP_STALE_SLACK_MS`, 5 min): stale recap segments render dim so an out-of-date
  recap doesn't masquerade as current. The token is never dimmed.

### What moves out of this column

The pushed status *message* text and the full question text no longer render in the
flex column — the token carries the state. Both remain visible in the PM pane and the
workspace-updates modal. This is a deliberate trade: keywords + status beat one long
message for at-a-glance scanning.

## 4. Agent doctrine

- `CLAUSE_RECAP` in `src/agent/doctrine.rs` and `skills/wsx/SKILL.md` are updated:
  agents maintain the short forms alongside the full fields whenever they set a recap.
- Convention: keyword distillation — identifiers, ticket/PR numbers, no filler words.
  Targets: goal-short ≤ ~40 chars, state-short and next-short ≤ ~24 chars. These are
  guidance for agents, not enforced limits.

## 5. Scope

Dashboard flex column only (`row.rs` consumers via `column_content.rs`, both by-repo
and by-attention views). The PM pane, macOS menubar PM submenu, and workspace-updates
modal are unchanged.

## 6. Testing

- Composer unit tests: token precedence (fresh push vs derived status), greedy segment
  dropping at several widths, each fallback tier (short → clipped full → today's text),
  stale dimming, whitespace collapsing.
- CLI parse tests for the new flags, including the at-least-one-flag rule.
- Store round-trip tests for the new columns, including partial upsert behavior.
- CI gates per repo convention: `cargo fmt --check`, clippy, `cargo test`.
