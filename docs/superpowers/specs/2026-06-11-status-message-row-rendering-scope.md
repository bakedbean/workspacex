# Follow-up scope: render the agent status message in the dashboard row

**Parent feature:** agent-driven status reporting (issue #166, PR #167).
**Status:** Implemented in this branch / PR #167. Decision: **row column only** (the always-visible glance value); detail-pane rendering deferred (see Out of scope).

## Why

PR #167 stores an agent-authored `message` (`workspace_status.message`) but renders
nothing — so the dashboard still shows only the 6-state glyph plus the existing
heuristic recap. The message is the highest-value *visible* part of the feature
("see what each agent is actually doing / why it's blocked"). This follow-up
surfaces it in the one place that delivers at-a-glance value: the per-workspace row.

## Key facts that make this small

- The data is already in memory: `App.pushed_status: HashMap<WorkspaceId, ReportedStatus>`
  (loaded in `refresh()`, `src/app.rs:432`).
- The freshness gate already exists: `fresh_reported_state(Option<&ReportedStatus>, i64)`
  (`src/app.rs`), used by `classify_status`.
- **A non-empty message is always model-authored.** Hooks write `message = None`
  (`StatusFromHook` handler in `src/cli.rs`); only the tier-1 `wsx status set --message`
  path writes a message. So "message present ⇒ deliberate, high-signal" — no need to
  distinguish source at render time.
- The row already has a flex "message" column (`src/ui/dashboard/row.rs:227-258`)
  that today shows the heuristic recap from `row_column` (`column_content.rs:33-99`).
  This is the natural home — we change what fills it, not the layout.

## Design

### Precedence
When a **fresh** pushed `ReportedStatus` has a non-empty (trimmed) message, the row's
message column shows that message. Otherwise `row_column`'s existing status-adaptive
heuristic runs unchanged. This holds across all states (including Complete/Idle —
showing "done · implemented the refactor" on a finished workspace is desirable). The
status *glyph* is unchanged: it still comes from `classify_status`, which already folds
the pushed state in (e.g. `blocked` → Question), so glyph and message stay consistent.

**No regression surface:** when nothing is pushed (every non-cooperating agent, all
non-Claude agents today), behavior is byte-for-byte identical to current.

### Freshness (reuse, don't duplicate)
Refactor so one gate serves both the state and the message:

```rust
// src/app.rs — new free fn; existing fresh_reported_state delegates to it.
pub(crate) fn fresh_reported<'a>(
    reported: Option<&'a ReportedStatus>,
    last_log_activity_ms: i64,
) -> Option<&'a ReportedStatus> {
    let r = reported?;
    (r.reported_at >= last_log_activity_ms).then_some(r)
}

pub(crate) fn fresh_reported_state(
    reported: Option<&ReportedStatus>,
    last_log_activity_ms: i64,
) -> Option<ReportedState> {
    fresh_reported(reported, last_log_activity_ms).map(|r| r.state)
}
```

Add an `App` accessor that applies the gate using the workspace's `last_log_activity_ms`
(same lookup `classify_status` already does):

```rust
pub fn fresh_reported_status(&self, ws_id: WorkspaceId) -> Option<&ReportedStatus> {
    let last = self.workspace_events.get(&ws_id)
        .map(|e| e.last_log_activity_ms).unwrap_or(0);
    fresh_reported(self.pushed_status.get(&ws_id), last)
}
```

The message thus appears under the exact same liveness rule as the glyph and disappears
the moment the agent produces new JSONL activity after reporting.

### Visual treatment
Agent messages must read as deliberate/current, distinct from the dim heuristic recap.
Add a `ColumnEmphasis::Reported` variant (`column_content.rs`). In `row.rs`'s message-column
block, when the column is `Reported`:
- prefix `▸ ` (instead of the heuristic `└ `),
- render the body in the row's status color (`theme.status_style(status)`) rather than dim.

Keep it subtle — one glyph + color, no layout/width change. Single-line, truncated to the
flex width as today (messages are short one-liners by design).

## Files to touch

| File | Change |
|------|--------|
| `src/app.rs` | Add `fresh_reported` free fn; `fresh_reported_state` delegates to it; add `App::fresh_reported_status` accessor. |
| `src/ui/dashboard/column_content.rs` | `row_column` takes the fresh message; returns a `RowColumn` with `ColumnEmphasis::Reported` when a message is present, else existing logic. Add the `Reported` emphasis variant. |
| `src/ui/dashboard/row.rs` | Render `ColumnEmphasis::Reported` with the `▸ ` prefix + status color. |
| `src/app/render.rs` | At the row build (`~110-114`), pass `app.fresh_reported_status(ws.id)`'s message into `row_column`. |

## Task outline (TDD, ~3 commits, ~half day)

1. **App accessor + gate refactor.** Add `fresh_reported` + `fresh_reported_status`;
   point `fresh_reported_state` at the new fn; keep `classify_status` behavior identical.
   Tests: the existing freshness tests still pass; add one asserting `fresh_reported`
   returns the ref on a tie (`reported_at == last_log_activity_ms`) and `None` after.
2. **`row_column` precedence + `Reported` emphasis.** When a non-empty message is passed,
   return it with `ColumnEmphasis::Reported`; else unchanged. Tests: message present
   overrides the heuristic recap (e.g. in Complete state, the message wins over
   `last_completed_turn_text`); message absent falls back to current text; empty/whitespace
   message falls back.
3. **`row.rs` rendering + `render.rs` wiring.** Render the `Reported` emphasis with the
   distinct prefix/color; thread `fresh_reported_status(ws.id)` through. Verify with any
   existing row-render/snapshot tests in the module; otherwise a focused unit test that the
   rendered spans use the status style + `▸ ` prefix for a `Reported` column.

## Out of scope
- Detail-pane (`session_summary.rs`) rendering — deferred (could be a later addition that
  shows the full message + state + age for the selected workspace).
- Surfacing `source` ("model"/"hook") in the UI — unnecessary while only model pushes carry messages.
- Any change to the glyph, state vocabulary, layout, or column widths.
- Non-Claude deterministic mechanisms (separate `StatusIntegration` work).

## Demonstrability
With this landed, `wsx status set blocked --message "need your call on the auth approach"`
shows, in that workspace's row, `▸ need your call on the auth approach` in the question color —
the deliberate, current, human-readable signal the heuristic recap can't produce. That is the
concrete experience improvement the parent feature was aiming at.
