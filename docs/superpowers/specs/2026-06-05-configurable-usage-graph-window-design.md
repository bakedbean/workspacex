# Configurable usage-graph window

**Date:** 2026-06-05
**Status:** Approved design, pending implementation
**Workspace:** `configurable-usage-graph-window`

## Summary

The dashboard footer shows a fixed 24-bar sparkline of recent activity, hardcoded
to the last 24 hourly buckets and labeled `24h`. This change makes the window
**user-configurable** to one of three spans — **24 hours**, **1 week**, or **1
month** — settable two ways:

1. Via the existing CLI settings system (`wsx config set usage_graph_window <value>`).
2. By **clicking the graph** in the footer, which opens a small picker anchored
   over the graph.

The sparkline stays a fixed 24 bars wide at every span; only the time-per-bar and
the label change.

## Goals

- Let the user choose the usage-graph window: 24h / 1w / 1mo.
- Keep the footer graph a constant width (no layout shift between spans).
- Make the window settable from the CLI and by clicking the graph.
- Update the graph live when the window changes (no restart).

## Non-goals

- No new keyboard shortcut to open the picker (click + CLI only).
- No per-repo override; this is a single global setting.
- No change to *what* activity is measured (still: peak concurrent
  Thinking/Waiting workspaces per hour).
- No configurable bar count or arbitrary custom spans (just the three options).

## Background: how it works today

- **Data:** A SQLite table `activity_buckets (hour_epoch INTEGER PK, max_live
  INTEGER)`. Each bucket holds the *max* number of concurrently active
  (Thinking/Waiting) workspaces seen during that clock hour.
  (`src/data/store.rs`)
- **In-memory:** `App.activity_history: VecDeque<(u64 hour_epoch, u32 max_live)>`.
  The tick loop (`src/app.rs:747-779`) maintains the current hour's bucket,
  rolls over on the hour, caps the deque at 24, and prunes the DB older than 24h.
- **Startup:** loads `recent_activity_buckets(24)` (`src/app.rs:332`).
- **Render:** `src/ui/dashboard/layout.rs:126-127` renders a fixed 24-char
  sparkline with a hardcoded `"24h"` label. `src/ui/dashboard/sparkline.rs`
  scales `u32` samples into 8 Unicode block levels and pads/truncates to a
  requested length.
- **Settings:** key/value `settings` table; accessed via
  `store.get_setting/set_setting`; CLI surface `wsx config get/set/list/edit`
  (`src/cli.rs:1166-1223`). Pattern: read string → parse → fall back to default
  on error (see `src/config/detail_bar_config.rs` for the warn-and-default
  pattern).
- **Mouse:** capture is enabled (`src/main.rs:78`). Clicks are dispatched in
  `handle_mouse` (`src/app/input.rs:1631-1708`) using a *draw-time-populates /
  input-time-reads* hit-test pattern: render stores `Rect`s on `App` (e.g.
  `chip_rects`), cleared each frame (`src/app/render.rs:26-34`), and the click
  handler does a simple bounds check.
- **Modals:** `Modal` enum on `App.modal` (`src/ui/modal.rs:25-77`). Most render
  centered via a `centered()` helper; some (`UpdatesPanel`, `ProcessList`) are
  special-cased and rendered directly from `app::render::draw` with custom
  placement. Input is gated by `app.modal.is_some()` and routed through
  `handle_key_modal` (`src/app/input.rs:986+`). `AgentPicker` is a near-identical
  "compact option list + ↑↓ + Enter" precedent.

## Decisions (resolved during brainstorming)

1. **Bar mapping — fixed 24 bars, aggregate.** The sparkline is always 24 bars;
   each bar's span scales with the window (1h / 7h / 30h). Width never shifts;
   resolution is constant. (Rejected: natural granularity — 24/7/30 bars —
   because it changes footer width and a 7-bar week looks sparse.)
2. **Aggregation — time-aligned, max.** Bars map to *real clock spans* ending
   "now", not to "the last N buckets". Each bar's value is the **max** of the
   hourly buckets falling in its span (consistent with how each hourly bucket is
   itself a max); empty spans render as 0 (lowest block). This is a slight,
   deliberate change from today's index-based behavior: idle/downtime periods now
   show honestly as low bars instead of collapsing.
3. **Retention — always keep 30 days.** Retain the maximum window's worth of
   hourly buckets regardless of the current setting, so the setting is purely a
   *view*; switching from `24h` to `1w` after a week of use immediately shows
   real history. Cost is ~720 tiny rows.
4. **Click interaction — anchored popup at the graph.** Clicking the graph opens
   a small menu anchored just above it (the one new bit of UI; all current modals
   are centered). (Rejected: centered modal — disconnected from the graph;
   click-to-cycle — not discoverable, can't jump.)

## Design

### 1. The setting and `UsageWindow` enum

New global setting key `usage_graph_window`, stored in the existing `settings`
table. Canonical values: `24h` (default), `1w`, `1mo` — these exact tokens only;
unrecognized values fall back to `24h`.

```rust
// e.g. src/config/usage_window.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageWindow { Day, Week, Month }

impl UsageWindow {
    pub const ALL: [UsageWindow; 3] = [Self::Day, Self::Week, Self::Month];

    /// Total span in hours: 24 / 168 / 720.
    pub fn hours(self) -> u64 { match self { Self::Day => 24, Self::Week => 168, Self::Month => 720 } }

    /// Compact footer label: "24h" / "1w" / "1mo".
    pub fn label(self) -> &'static str { match self { Self::Day => "24h", Self::Week => "1w", Self::Month => "1mo" } }

    /// Parse a canonical token ("24h"|"1w"|"1mo"); anything else → Day.
    pub fn from_setting(s: &str) -> UsageWindow { /* match exact canonical tokens */ }

    /// Canonical token for persistence.
    pub fn as_setting(self) -> &'static str { self.label() }

    pub fn index(self) -> usize { /* position in ALL */ }
    pub fn from_index(i: usize) -> UsageWindow { /* ALL[i.min(2)] */ }
}

/// Read the configured window from the store, defaulting to Day.
pub fn resolve(store: &Store) -> UsageWindow {
    match store.get_setting("usage_graph_window") {
        Ok(Some(s)) => UsageWindow::from_setting(&s),
        _ => UsageWindow::Day,
    }
}
```

The window is **read at render time** (single-key SQLite read, cheap and
precedented by `pinned_commands`), so CLI changes and picker selections both take
effect on the next frame with no cache to invalidate.

### 2. Time-aligned aggregation

A pure function collapses the retained hourly buckets into exactly 24 samples:

```rust
// e.g. src/ui/dashboard/sparkline.rs or a sibling module
/// Aggregate hourly (hour_epoch, max_live) buckets into `bars` samples covering
/// the most recent `window_hours`, ending at `now_hour`. Each output bar spans
/// `window_hours / bars` hours and takes the MAX of buckets in its span; spans
/// with no buckets yield 0.
pub fn aggregate_buckets(
    buckets: &[(u64, u32)],
    now_hour: u64,
    window_hours: u64,
    bars: usize,
) -> Vec<u32>;
```

- Span per bar = `window_hours / bars` (1 / 7 / 30 hours for 24 bars).
- Bar `i` (0-based, oldest first) covers
  `[now_hour - (bars - i) * span * 3600, now_hour - (bars - 1 - i) * span * 3600)`.
- Value = max `max_live` over buckets whose `hour_epoch` is in that range, else 0.
- Output length is always `bars` (24).

The result feeds the existing `sparkline::render(samples, 24)` unchanged.

### 3. Footer label and graph hit-rect

- `footer()` (`src/ui/dashboard/layout.rs`) gains a `window_label: &str`
  parameter replacing the hardcoded `"24h"`; sparkline width stays 24.
- `footer()` / `render_footer` returns the **width of the trailing graph
  segment** (the `"<label> <spark>"` portion) so the draw loop can derive the
  graph's on-screen `Rect`.
- The draw loop (`src/app/render.rs`) computes and stores
  `app.usage_graph_rect: Option<Rect>` covering that segment, cleared each frame
  alongside the other rect fields.

`app/render.rs` flow per frame:
1. `let window = usage_window::resolve(&app.store);`
2. `let samples = aggregate_buckets(&history, now_hour, window.hours(), 24);`
3. Pass `samples` + `window.label()` to the footer; store the returned graph rect.

### 4. Retention to 30 days

Replace the hardcoded `24` retention horizon with a constant
`const MAX_ACTIVITY_HOURS: u64 = 720;` (30 days), applied in `src/app.rs`:

- Startup load: `recent_activity_buckets(720)` (was 24) — line ~332.
- In-memory cap: `while activity_history.len() > 720` (was 24) — line ~774.
- Prune cutoff: `now_hour.saturating_sub(720 * 3600)` (was `24 * 3600`) —
  line ~777.

No schema change; the `activity_buckets` table already keys by `hour_epoch`.

### 5. Clickable graph + anchored picker

**New modal variant** (`src/ui/modal.rs`):

```rust
Modal::UsageWindowPicker { selected: usize }   // selected indexes UsageWindow::ALL
```

**Opening** — in the left-click branch of `handle_mouse`
(`src/app/input.rs:~1674`), add a bounds check against `app.usage_graph_rect`
(same inline pattern as `chip_rects`). On hit, when no modal is open:
`app.modal = Some(Modal::UsageWindowPicker { selected: window.index() })`.

**Anchored rendering** — rendered directly from `app::render::draw` (not via
`centered()`), mirroring how `UpdatesPanel`/`ProcessList` are special-cased. A
pure helper positions the box:

```rust
/// Position a (w x h) popup just above the footer, left-aligned to `graph_rect`,
/// clamped to stay fully within `screen`.
fn picker_rect(graph_rect: Rect, screen: Rect, w: u16, h: u16) -> Rect;
```

The popup draws `Clear` + a bordered block listing `24h / 1w / 1mo`, marking the
current window with `•` and the cursor selection via highlight. Each option's row
`Rect` is stored (e.g. `app.usage_window_option_rects: Vec<Rect>`, cleared each
frame) so options are mouse-clickable.

**Interaction** — once open, input is gated by `app.modal.is_some()`:
- `handle_key_modal` arm for `UsageWindowPicker`: **↑↓** move `selected` (wraps),
  **Enter** applies `UsageWindow::from_index(selected)`, **Esc** closes.
- In `handle_mouse`: a click inside an option row applies that option; a click
  outside the popup closes it (dismiss).
- **Apply** = `store.set_setting("usage_graph_window", win.as_setting())` then
  `app.modal = None`. The graph reflects it on the next frame via the render-time
  read.

### Data flow (per frame)

```
settings table ──resolve()──> UsageWindow ──hours()──┐
activity_history (≤720 buckets) ─────────────────────┤
                                                      ▼
                              aggregate_buckets(.., 24) ──> [u32; 24]
                                                      │            │
                                  window.label() ─────┘            ▼
                                                      footer(samples, label)
                                                      │
                                          returns graph-segment width
                                                      ▼
                                       app.usage_graph_rect (for clicks)
```

## Error handling

- Unknown/garbage setting value → `UsageWindow::Day` (lenient parse; optionally a
  `tracing::warn!`, matching `detail_bar_config`).
- `store.get_setting` error → default `Day`.
- `set_setting` failure on apply → log; popup still closes (best-effort, matches
  existing settings writes which ignore errors).
- Narrow/short terminal: `picker_rect` clamps on-screen; if the graph rect is
  clipped off-screen, clicks simply don't register (acceptable).
- Empty history / no buckets in a span → 0 → lowest block (existing sparkline
  behavior).

## Testing

Pure functions, unit-tested:

- `aggregate_buckets`:
  - 24h window, one bucket per hour → 24 bars equal to inputs (max semantics).
  - 1w window → buckets grouped into 7h spans, each bar = max of its span.
  - Empty spans (gaps/downtime) render as 0.
  - Output length always 24 for every window.
- `UsageWindow`: `from_setting` canonical-token mapping; unknown → `Day`;
  `hours()`/`label()`/`as_setting()` round-trip; `index()`/`from_index()` inverse.
- `picker_rect`: graph near right edge → clamped within screen width; near top →
  clamped; normal case → sits directly above the footer, left-aligned to graph.
- Click-opens-picker: a click within a stored `usage_graph_rect` yields the
  picker modal with `selected` = current index (bounds-check logic).

Existing `sparkline.rs` tests are unchanged (still renders 24).

## Files touched

| File | Change |
|------|--------|
| `src/config/usage_window.rs` (new) | `UsageWindow` enum + `resolve()` |
| `src/config/mod.rs` | register module |
| `src/ui/dashboard/sparkline.rs` (or sibling) | `aggregate_buckets()` + tests |
| `src/ui/dashboard/layout.rs` | `footer()` takes `window_label`, returns graph-segment width |
| `src/ui/dashboard/mod.rs` | thread label/width through `render_footer` |
| `src/app.rs` | `MAX_ACTIVITY_HOURS = 720`; startup load / cap / prune; new rect fields |
| `src/app/render.rs` | resolve window, aggregate, store `usage_graph_rect`; render anchored picker; clear new rects |
| `src/app/input.rs` | open picker on graph click; `handle_key_modal` arm; option-click / click-outside |
| `src/ui/modal.rs` | `Modal::UsageWindowPicker` variant |

CLI needs **no** new code — `wsx config set usage_graph_window 1w` works via the
existing generic settings commands.

## Rollout / compatibility

- No migration: absent setting → default `24h`; existing `activity_buckets` data
  is forward-compatible.
- The only behavior change for users who never touch the setting is the
  index→time-aligned 24h rendering, visible only when wsx had downtime (gaps now
  show as low bars rather than collapsing).
