# Status-adaptive workspace row column

## Problem

Each workspace row in the dashboard ends with a flex-width column (the
widest, most adaptive space in the row — everything else is fixed-width).
Today it renders `last_message`: the most recent assistant text block from
the agent's session. In practice this is low-signal — it's whatever the
agent last said, which is often mid-thought chatter that doesn't help you
decide which workspace needs you.

Meanwhile the data layer already tails a rich set of per-workspace signals
into `WorkspaceEvents` (`first_user_text`, `tool_use_counts`,
`pending_tool_uses`, `last_completed_turn_text`, stall timing) that the
detail bar's SESSION SUMMARY column synthesizes into useful one-liners —
but the row itself never uses them.

## Goal

Replace the static "last agent message" with a **status-adaptive column**:
its content is a function of the row's canonical `Status`, so scanning the
list top-to-bottom becomes pure triage — you instantly see who is blocked
on you, who is making progress, and who is done.

Non-goals: no new data plumbing (every signal already exists in
`WorkspaceEvents`); no per-user configurability of the mapping (a possible
future follow-on, explicitly out of scope here); no change to the
fixed-width columns or the detail bar.

## Content mapping

The column content is chosen by the row's `Status` (the canonical 6-state
enum in `src/ui/dashboard/status.rs`):

> **Note (as-built):** the table below reflects the shipped implementation.
> Per-status emoji glyphs (`❓ ⚠ ✓ ⌖`) were intentionally **dropped** — they
> are double-width in many terminals and would misalign the right-aligned age
> column against the row's `chars()`-based truncation. The existing `└ ` prefix
> and per-status status glyph (column 3) carry that signal instead; color
> emphasis distinguishes the attention states.

| Status | Column content | Source signal | Emphasis |
|--------|----------------|---------------|----------|
| **Question** | `AskUserQuestion` / `ExitPlanMode` / pending permission tool name (e.g. `Bash`) | `pending_question_tool()`, falling back to `pending_permission_tool()` | status color |
| **Stalled** | `stalled · 4m quiet` | stall branch of `format_state_line` (quiet duration from `last_log_activity_ms`) | warn color |
| **Waiting** (>30s idle) | `edited 3 files, ran 2 commands` | `format_tool_trace(tool_use_counts)` | dim |
| **Thinking** (<30s idle) | `read 14 files` / `edited 3 files, ran 2 commands` | `format_tool_trace(tool_use_counts)` | dim |
| **Complete** | `split the quick-start into two…` | `last_completed_turn_text` (cleaned, pinned) | dim |
| **Idle** | `backfill the 003 migration table` | `first_user_text` | dim |

The tool trace is comma-separated (`format_tool_trace`), not dot-separated.

Decisions confirmed with the user:

- **Color:** only the attention states get color — `Question` in its status
  color, `Stalled` in the warn/err color. Every other state stays dim, as
  the message line is today. This draws the eye to rows that need action
  without making a long list loud.
- **Complete state:** show the cleaned turn recap (`last_completed_turn_text`),
  not PR status (lifecycle already shows as the branch glyph) and not a raw
  tool tally.

### Fallbacks (signal absent)

A status may be reached before its preferred signal exists (e.g. a turn that
has produced no tool_use yet, or events not yet tailed). The column degrades
gracefully rather than rendering an empty or misleading cell:

- **Question** with neither a question nor permission tool pending →
  bare label `question`.
- **Thinking / Waiting** with empty `tool_use_counts` → `thinking…` /
  `waiting…` (the status label with an ellipsis), matching today's
  "just started" feel.
- **Complete** with a blank/whitespace-only `last_completed_turn_text` →
  fall back to `first_user_text` (the intent), then to the em-dash if even
  that is empty after trimming. Each candidate is trimmed and emptiness-checked
  independently so a blank recap never blocks the prompt fallback.
- **Idle** with no (or blank) `first_user_text` → em-dash (current behavior
  for an empty message).
- Events not yet scanned (`events` is `None`) → em-dash, as today.

No per-status glyph is rendered (see the as-built note above): the column
shows only the synthesized text, prefixed by the unchanged `└ ` and colored
by emphasis. Raw `first_user_text` / recap text is run through `collapse_ws`
so interior newlines don't break the single-line row layout.

## Architecture

The synthesized string is computed **at the single production construction
site** where `RowInputs` is built — the workspace loop in `src/app/render.rs`,
which is the only place that builds production rows and where both
`WorkspaceEvents` and a frame-level `now_ms` time base are available. (`now_ms`
is hoisted once per frame there and reused for both the column's stall clock
and the row's age clock so they can't diverge.) `row.rs::render` stays pure: it
receives a precomputed value and only lays it out and colors it. This preserves
the existing property that `row.rs` has no dependency on classifier internals
or wall-clock time.

### Data shape

Replace the `last_message: Option<String>` field on `RowInputs` with:

```rust
/// Precomputed flex-column content, chosen by the caller from the
/// workspace's status + events. `None` renders as the em-dash.
pub column: Option<RowColumn>,

pub struct RowColumn {
    pub text: String,
    pub emphasis: ColumnEmphasis,
}

pub enum ColumnEmphasis {
    Dim,        // default — all non-attention states
    Status,     // Question — paint in the row's status color
    Warn,       // Stalled
}
```

`row.rs` maps `ColumnEmphasis` to a concrete `Style` via the existing
`Theme` (`dim_style`, `status_style(status)`, the warn/err style), keeping
all color decisions in the theme.

### Shared synthesizer module

`format_tool_trace`, `format_state_line`, `format_recent_files`, and the
`format_ago_short` / `truncate_to_chars` helpers currently live private to
`src/detail_modules/session_summary.rs`. Extract the reusable pieces into a
new module (e.g. `src/activity/summary.rs` or
`src/ui/dashboard/column_content.rs`) so both the detail bar and the row
call the same code and cannot drift. `session_summary.rs` becomes a consumer
of that module. The new module exposes one entry point:

```rust
/// Build the status-adaptive row column for a workspace.
/// `now_ms` is the shared epoch-ms time base (same one app.rs uses).
pub fn row_column(
    status: Status,
    events: Option<&WorkspaceEvents>,
    now_ms: i64,
) -> Option<RowColumn>;
```

(There is no `nerd_fonts` parameter: since the as-built column renders no
per-status glyph, the synthesizer needs no font-capability input.)

This function owns the mapping table and the fallback ladder above. It is
the single place to unit-test the state→content logic.

## Data flow

```
app state (WorkspaceEvents per ws)
  │
  ├─ app/render.rs  (workspace loop builds RowInputs, hoists frame now_ms)
  │     └─ column_content::row_column(status, events, now_ms)
  │           → Option<RowColumn>
  │
  └─ row.rs::render(inputs, …)
        └─ lays out inputs.column.text in the flex slot,
           styled by inputs.column.emphasis (via Theme)
```

## Search / filter interaction

`matches_filter` in `mod.rs` currently matches the filter needle against
`last_message`. After this change, match against `RowColumn.text` instead so
filtering still searches the visible column. (Optionally also keep matching
`first_user_text` so a parked Idle workspace stays findable by its original
prompt regardless of which state it is in — decide during implementation;
default is to match the rendered text only, for predictability.)

## Error handling / edge cases

- **Truncation:** the existing flex-width truncation in `row.rs` is unchanged
  — the precomputed `text` is truncated to fit and right-padded exactly as
  `last_message` is today, including the `└ ` prefix treatment. (Open
  implementation detail: whether the per-status glyph replaces or augments
  the existing `└ ` prefix — resolve when wiring `row.rs`, keeping total
  flex width math intact.)
- **Time base:** callers must pass the same `now_ms` derived from
  `SystemTime::now()` that `app.rs` / `background.rs` use (millis, not
  `secs * 1000`), so stall durations match the detail bar.
- **Status flicker:** content changes with status, which already changes on
  its own cadence; no new flicker source is introduced beyond what the
  status glyph/gutter already exhibit.

## Testing

- **`column_content::row_column` unit tests** (the bulk): one test per status
  asserting the chosen signal renders, plus one per fallback rung (Question
  with no pending tool, Thinking with empty counts, Complete with no recap →
  intent → em-dash, Idle with no intent, events `None`). Mirror the existing
  `session_summary.rs` test style (`Box::leak` a `WorkspaceEvents`, seed
  `pending_tool_uses` timestamps at epoch 0 to clear the 3s threshold).
- **`row.rs` render tests:** assert each `ColumnEmphasis` maps to the
  expected style and that the column text lands in the flex slot at a couple
  of terminal widths (reuse the existing `TestBackend` render harness).
- **Regression:** the `by_attention` / `by_repo` fixtures and snapshot-style
  tests update to feed `column` instead of `last_message`.

## Out of scope / future

- User-configurable per-status mapping (mirroring the detail-bar module
  registry). The extracted `column_content` module makes this a clean future
  addition, but it is not built here.
- PR/forge status in the column. Lifecycle stays a branch glyph for now.
