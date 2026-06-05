# Configurable Usage-Graph Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the dashboard footer's activity sparkline window user-configurable (24h / 1 week / 1 month), settable both via `wsx config set usage_graph_window <value>` and by clicking the graph to open an anchored picker.

**Architecture:** A new `UsageWindow` enum + a pure `aggregate_buckets` function collapse the retained hourly activity buckets into a fixed 24-bar, time-aligned sparkline whose span scales with the chosen window. Retention grows to 30 days so the setting is purely a view. A new `Modal::UsageWindowPicker` renders anchored above the graph (not centered like other modals), opened by a click on the stored graph `Rect`. The window is read from the `settings` table at render time, so CLI and picker changes both take effect on the next frame.

**Tech Stack:** Rust, ratatui + crossterm TUI, SQLite (`rusqlite`) key/value `settings` table.

**Reference spec:** `docs/superpowers/specs/2026-06-05-configurable-usage-graph-window-design.md`

---

## File Structure

| File | Responsibility | New/Modified |
|------|----------------|--------------|
| `src/config/usage_window.rs` | `UsageWindow` enum (hours/label/parse/index) + `resolve(store)` | **New** |
| `src/config/mod.rs` | register `usage_window` module | Modify |
| `src/ui/dashboard/sparkline.rs` | add pure `aggregate_buckets()` + tests | Modify |
| `src/ui/dashboard/layout.rs` | `footer()` takes `window_label`, returns `(Line, graph_width)` | Modify |
| `src/ui/dashboard/mod.rs` | `render_footer()` takes label, returns graph `Rect`; fix dead `render()` call | Modify |
| `src/app.rs` | `MAX_ACTIVITY_HOURS=720` retention; new `App` rect fields + init | Modify |
| `src/app/render.rs` | resolve window, aggregate, store graph rect, clear new rects, render anchored picker | Modify |
| `src/ui/modal.rs` | `Modal::UsageWindowPicker`; `picker_rect()` + `render_usage_window_picker()`; guard | Modify |
| `src/app/input.rs` | open picker on graph click; option-click / outside-close; `handle_key_modal` arm | Modify |

**Test commands:** `cargo test` (unit tests), `cargo clippy --all-targets -- -D warnings`, `cargo build`.

---

## Task 1: `UsageWindow` enum + `resolve()`

**Files:**
- Create: `src/config/usage_window.rs`
- Modify: `src/config/mod.rs` (add `pub mod usage_window;` after `pub mod detail_bar_config;`)

- [ ] **Step 1: Write the failing tests**

Create `src/config/usage_window.rs` with only the test module first (the types come in Step 3):

```rust
//! The dashboard footer's activity-graph window: 24h / 1 week / 1 month.
//!
//! Stored in the `settings` table under key `usage_graph_window` as one of the
//! canonical tokens "24h" | "1w" | "1mo". Read at render time so CLI (`wsx
//! config set usage_graph_window 1w`) and the in-app picker both apply live.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_and_label_per_variant() {
        assert_eq!(UsageWindow::Day.hours(), 24);
        assert_eq!(UsageWindow::Week.hours(), 168);
        assert_eq!(UsageWindow::Month.hours(), 720);
        assert_eq!(UsageWindow::Day.label(), "24h");
        assert_eq!(UsageWindow::Week.label(), "1w");
        assert_eq!(UsageWindow::Month.label(), "1mo");
    }

    #[test]
    fn from_setting_accepts_canonical_tokens_only() {
        assert_eq!(UsageWindow::from_setting("24h"), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting("1w"), UsageWindow::Week);
        assert_eq!(UsageWindow::from_setting("1mo"), UsageWindow::Month);
        // Anything else falls back to the default (Day).
        assert_eq!(UsageWindow::from_setting("week"), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting("1d"), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting(""), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting("garbage"), UsageWindow::Day);
    }

    #[test]
    fn as_setting_round_trips_through_from_setting() {
        for w in UsageWindow::ALL {
            assert_eq!(UsageWindow::from_setting(w.as_setting()), w);
        }
    }

    #[test]
    fn index_and_from_index_are_inverse() {
        for (i, w) in UsageWindow::ALL.iter().enumerate() {
            assert_eq!(w.index(), i);
            assert_eq!(UsageWindow::from_index(i), *w);
        }
        // Out-of-range index clamps to the last variant.
        assert_eq!(UsageWindow::from_index(99), UsageWindow::Month);
    }
}
```

Add the module registration in `src/config/mod.rs`:

```rust
pub mod detail_bar_config;
pub mod usage_window;
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test --lib config::usage_window`
Expected: FAIL — `cannot find type UsageWindow in this scope` (types not defined yet).

- [ ] **Step 3: Write the minimal implementation**

Insert above the `#[cfg(test)]` module in `src/config/usage_window.rs`:

```rust
use crate::data::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageWindow {
    Day,
    Week,
    Month,
}

impl UsageWindow {
    pub const ALL: [UsageWindow; 3] = [Self::Day, Self::Week, Self::Month];

    /// Total span in hours: 24 / 168 / 720.
    pub fn hours(self) -> u64 {
        match self {
            Self::Day => 24,
            Self::Week => 168,
            Self::Month => 720,
        }
    }

    /// Compact footer label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "24h",
            Self::Week => "1w",
            Self::Month => "1mo",
        }
    }

    /// Canonical token used for persistence (same as `label`).
    pub fn as_setting(self) -> &'static str {
        self.label()
    }

    /// Parse a canonical token; anything unrecognized falls back to `Day`.
    pub fn from_setting(s: &str) -> UsageWindow {
        match s {
            "24h" => Self::Day,
            "1w" => Self::Week,
            "1mo" => Self::Month,
            _ => Self::Day,
        }
    }

    /// Position within `ALL` (used to seed/apply the picker selection).
    pub fn index(self) -> usize {
        match self {
            Self::Day => 0,
            Self::Week => 1,
            Self::Month => 2,
        }
    }

    /// Inverse of `index`; out-of-range clamps to the last variant.
    pub fn from_index(i: usize) -> UsageWindow {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }
}

/// Read the configured window from the store, defaulting to `Day` on missing
/// key, parse failure, or DB error.
pub fn resolve(store: &Store) -> UsageWindow {
    match store.get_setting("usage_graph_window") {
        Ok(Some(s)) => UsageWindow::from_setting(&s),
        _ => UsageWindow::Day,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib config::usage_window`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/config/usage_window.rs src/config/mod.rs
git commit -m "feat(config): add UsageWindow enum for configurable graph window"
```

---

## Task 2: Pure `aggregate_buckets()` for time-aligned 24-bar sparkline

**Files:**
- Modify: `src/ui/dashboard/sparkline.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `src/ui/dashboard/sparkline.rs` (after the existing tests, before the closing `}`):

```rust
    // Helper: an hour-aligned "now" for deterministic bucket math.
    fn now_hour() -> u64 {
        1_000_000 - (1_000_000 % 3600)
    }

    #[test]
    fn day_window_maps_one_bucket_per_bar() {
        let now = now_hour();
        // 24 buckets, one per hour, oldest first, values 1..=24.
        let buckets: Vec<(u64, u32)> = (0..24)
            .map(|i| (now - (23 - i) as u64 * 3600, (i + 1) as u32))
            .collect();
        let out = aggregate_buckets(&buckets, now, 24, 24);
        assert_eq!(out, (1..=24).collect::<Vec<u32>>());
    }

    #[test]
    fn week_window_places_recent_bucket_in_last_bar() {
        let now = now_hour();
        let out = aggregate_buckets(&[(now, 5)], now, 168, 24);
        assert_eq!(out.len(), 24);
        assert_eq!(out[23], 5);
        assert!(out[..23].iter().all(|&v| v == 0));
    }

    #[test]
    fn aggregation_takes_max_within_a_span() {
        let now = now_hour();
        // Two buckets within the last 7h span of a 1-week window.
        let out = aggregate_buckets(&[(now, 3), (now - 3600, 7)], now, 168, 24);
        assert_eq!(out[23], 7);
    }

    #[test]
    fn buckets_older_than_window_are_ignored() {
        let now = now_hour();
        let out = aggregate_buckets(&[(now - 200 * 3600, 9)], now, 168, 24);
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn output_length_is_always_bars() {
        let now = now_hour();
        assert_eq!(aggregate_buckets(&[], now, 24, 24).len(), 24);
        assert_eq!(aggregate_buckets(&[], now, 168, 24).len(), 24);
        assert_eq!(aggregate_buckets(&[], now, 720, 24).len(), 24);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib dashboard::sparkline`
Expected: FAIL — `cannot find function aggregate_buckets`.

- [ ] **Step 3: Write the minimal implementation**

Add this public function in `src/ui/dashboard/sparkline.rs` (after `render`, before the test module):

```rust
/// Collapse hourly `(hour_epoch, max_live)` buckets into `bars` samples covering
/// the most recent `window_hours`, ending at the end of `now_hour`. Each output
/// bar spans `window_hours / bars` hours and takes the MAX of the buckets whose
/// `hour_epoch` falls inside it; spans with no buckets yield 0. Output length is
/// always `bars`. Bar 0 is oldest, bar `bars-1` is most recent.
pub fn aggregate_buckets(
    buckets: &[(u64, u32)],
    now_hour: u64,
    window_hours: u64,
    bars: usize,
) -> Vec<u32> {
    let bars = bars.max(1);
    let span_hours = (window_hours / bars as u64).max(1);
    let span_secs = span_hours * 3600;
    let total_secs = span_secs * bars as u64;
    // Window ends at the end of the current hour so "now" lands in the last bar.
    let window_end = now_hour.saturating_add(3600);
    let window_start = window_end.saturating_sub(total_secs);

    let mut out = vec![0u32; bars];
    for &(hour, value) in buckets {
        if hour < window_start || hour >= window_end {
            continue;
        }
        let idx = ((hour - window_start) / span_secs) as usize;
        if idx < bars && value > out[idx] {
            out[idx] = value;
        }
    }
    out
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib dashboard::sparkline`
Expected: PASS (existing tests + 5 new).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/sparkline.rs
git commit -m "feat(dashboard): add time-aligned bucket aggregation for sparkline"
```

---

## Task 3: Dynamic footer label + graph hit-rect plumbing

**Files:**
- Modify: `src/ui/dashboard/layout.rs:89-133` (`footer` signature + return)
- Modify: `src/ui/dashboard/mod.rs:142-152` (`render_footer`) and `:89` (dead `render`)

- [ ] **Step 1: Write the failing test**

Add this test inside the existing `#[cfg(test)] mod tests` block in `src/ui/dashboard/layout.rs` (there is already a `text(&Line)` helper there):

```rust
    #[test]
    fn footer_uses_provided_window_label_and_reports_graph_width() {
        let theme = Theme::by_name("wsx");
        let (line, graph_w) = footer(&[1, 2, 3], "9.9.9", 120, &theme, "1w");
        let rendered = text(&line);
        assert!(rendered.contains("1w"), "label should appear: {rendered}");
        assert!(!rendered.contains("24h"), "old hardcoded label gone");
        // graph segment = label chars + 1 space + 24 sparkline chars.
        assert_eq!(graph_w, ("1w".chars().count() + 1 + 24) as u16);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib dashboard::layout`
Expected: FAIL — `footer` takes 4 args / returns `Line`, not `(Line, u16)`.

- [ ] **Step 3: Implement — change `footer` signature, label, and return**

In `src/ui/dashboard/layout.rs`, change the `footer` signature and its final lines. Replace:

```rust
pub fn footer(
    activity_samples: &[u32],
    version: &str,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
```

with:

```rust
pub fn footer(
    activity_samples: &[u32],
    version: &str,
    width: usize,
    theme: &Theme,
    window_label: &str,
) -> (Line<'static>, u16) {
```

Then replace the tail (currently lines ~126-132):

```rust
    let spark = sparkline::render(activity_samples, 24);
    let right = format!("{version}  24h {spark}");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
    Line::from(spans)
}
```

with:

```rust
    let spark = sparkline::render(activity_samples, 24);
    let right = format!("{version}  {window_label} {spark}");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
    // The clickable graph is the trailing "<label> <24-char sparkline>" run.
    let graph_w = (window_label.chars().count() + 1 + 24) as u16;
    (Line::from(spans), graph_w)
}
```

- [ ] **Step 4: Implement — `render_footer` takes label, returns graph `Rect`**

In `src/ui/dashboard/mod.rs`, replace `render_footer` (lines ~140-152):

```rust
/// Render only the footer line (key hints + sparkline) into `area`.
/// `area` should be exactly 1 row tall.
pub fn render_footer(f: &mut Frame, area: Rect, activity: &[u32], theme: &Theme) {
    f.render_widget(
        Paragraph::new(layout::footer(
            activity,
            env!("CARGO_PKG_VERSION"),
            area.width as usize,
            theme,
        )),
        area,
    );
}
```

with:

```rust
/// Render only the footer line (key hints + sparkline) into `area`.
/// `area` should be exactly 1 row tall. Returns the on-screen `Rect` of the
/// clickable activity graph (the trailing "<label> <sparkline>" run), so the
/// caller can hit-test clicks on it.
pub fn render_footer(
    f: &mut Frame,
    area: Rect,
    activity: &[u32],
    theme: &Theme,
    window_label: &str,
) -> Rect {
    let (line, graph_w) = layout::footer(
        activity,
        env!("CARGO_PKG_VERSION"),
        area.width as usize,
        theme,
        window_label,
    );
    f.render_widget(Paragraph::new(line), area);
    // The graph is right-aligned within the footer row.
    let x = area.x + area.width.saturating_sub(graph_w);
    Rect {
        x,
        y: area.y,
        width: graph_w.min(area.width),
        height: 1,
    }
}
```

- [ ] **Step 5: Fix the dead `render()` call site so it compiles**

In `src/ui/dashboard/mod.rs:89` (inside the unused `render()` fn), the call passes 4 args and ignores the return. Replace:

```rust
    render_footer(f, chunks[1], inputs.activity, theme);
```

with:

```rust
    let _ = render_footer(f, chunks[1], inputs.activity, theme, "24h");
```

- [ ] **Step 6: Run the test + build to verify**

Run: `cargo test --lib dashboard::layout`
Expected: PASS.
Run: `cargo build`
Expected: FAILS at `src/app/render.rs:347` (the live `render_footer` call still uses the old signature) — this is fixed in Task 6. If you are running tasks strictly in order, instead verify with: `cargo build 2>&1 | grep -c "render_footer"` and confirm the only error is at `app/render.rs`.

- [ ] **Step 7: Commit**

```bash
git add src/ui/dashboard/layout.rs src/ui/dashboard/mod.rs
git commit -m "feat(dashboard): footer takes window label, returns graph rect"
```

---

## Task 4: Grow activity retention to 30 days

**Files:**
- Modify: `src/app.rs` (add const; lines ~332, ~774, ~777)

- [ ] **Step 1: Add the retention constant**

Near the top of `src/app.rs` (module level, after the imports / before `pub struct App`), add:

```rust
/// How many hourly activity buckets to retain, in memory and in the DB. Sized
/// to the largest selectable usage-graph window (30 days), so the setting is
/// purely a view over already-collected data rather than affecting retention.
const MAX_ACTIVITY_HOURS: u64 = 720;
```

- [ ] **Step 2: Use it at the startup load (line ~332)**

Replace:

```rust
        if let Ok(buckets) = app.store.recent_activity_buckets(24) {
```

with:

```rust
        if let Ok(buckets) = app.store.recent_activity_buckets(MAX_ACTIVITY_HOURS as usize) {
```

- [ ] **Step 3: Use it for the in-memory cap (line ~774)**

Replace:

```rust
                        while g.activity_history.len() > 24 {
```

with:

```rust
                        while g.activity_history.len() > MAX_ACTIVITY_HOURS as usize {
```

- [ ] **Step 4: Use it for the DB prune cutoff (line ~777)**

Replace:

```rust
                        let _ = g.store.prune_activity_buckets_before(now_hour.saturating_sub(24 * 3600));
```

with:

```rust
                        let _ = g.store.prune_activity_buckets_before(
                            now_hour.saturating_sub(MAX_ACTIVITY_HOURS * 3600),
                        );
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | grep -E "app\.rs.*activity|MAX_ACTIVITY"`
Expected: no errors referencing these lines (the only remaining build error is still the `render_footer` call from Task 3, fixed in Task 6).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): retain 30 days of activity buckets for graph window"
```

---

## Task 5: Add `App` hit-rect fields for graph + picker options

**Files:**
- Modify: `src/app.rs` (struct fields ~241; constructor ~301)
- Modify: `src/app/render.rs` (per-frame clear ~26-34)

- [ ] **Step 1: Add the struct fields**

In `src/app.rs`, immediately after the `agent_chip_rects` field (line ~241), add:

```rust
    /// Rect of the footer activity graph from the last draw, used by
    /// `handle_mouse` to open the usage-window picker on click. `None` when the
    /// footer is not currently drawn. Mirrors the `chip_rects` draw-populates /
    /// input-reads pattern; reset each frame.
    pub usage_graph_rect: Option<ratatui::layout::Rect>,
    /// Per-option row rects of the open usage-window picker, in `UsageWindow::ALL`
    /// order, consumed by `handle_mouse` to apply a clicked option. Cleared each
    /// frame; only populated while the picker modal is open.
    pub usage_window_option_rects: Vec<ratatui::layout::Rect>,
```

- [ ] **Step 2: Initialize them in the constructor**

In `src/app.rs`, find the struct-literal initialization near `activity_history: std::collections::VecDeque::new(),` (line ~301) and add alongside it:

```rust
            usage_graph_rect: None,
            usage_window_option_rects: Vec::new(),
```

- [ ] **Step 3: Clear them at the top of each frame**

In `src/app/render.rs`, after `app.agent_chip_rects.clear();` (line ~34), add:

```rust
    app.usage_graph_rect = None;
    app.usage_window_option_rects.clear();
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | grep -E "usage_graph_rect|usage_window_option_rects"`
Expected: no errors referencing these fields (only the Task-3 `render_footer` error remains, fixed next).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/app/render.rs
git commit -m "feat(app): add hit-rect fields for usage graph and picker options"
```

---

## Task 6: Wire aggregation + dynamic label + graph rect into the live footer

**Files:**
- Modify: `src/app/render.rs` (build `activity` ~204; `render_footer` call ~347)

- [ ] **Step 1: Replace the raw activity collection with aggregation**

In `src/app/render.rs`, replace line ~204:

```rust
            let activity: Vec<u32> = app.activity_history.iter().map(|(_h, m)| *m).collect();
```

with:

```rust
            // Aggregate the retained hourly buckets into a fixed 24-bar,
            // time-aligned sparkline for the configured window.
            let window = crate::config::usage_window::resolve(&app.store);
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let now_hour = now_secs - (now_secs % 3600);
            let history: Vec<(u64, u32)> = app.activity_history.iter().copied().collect();
            let activity: Vec<u32> = crate::ui::dashboard::sparkline::aggregate_buckets(
                &history,
                now_hour,
                window.hours(),
                24,
            );
```

- [ ] **Step 2: Update the live `render_footer` call to pass the label and store the graph rect**

In `src/app/render.rs`, replace line ~347:

```rust
            dashboard::render_footer(f, footer_area, &activity, &app.theme);
```

with:

```rust
            let graph_rect =
                dashboard::render_footer(f, footer_area, &activity, &app.theme, window.label());
            app.usage_graph_rect = Some(graph_rect);
```

> Note: `window` is in scope here because it was bound in Step 1 within the same `View::Dashboard` match arm.

- [ ] **Step 3: Verify the whole crate now builds and all tests pass**

Run: `cargo build`
Expected: SUCCESS (Task 3's pending error is now resolved).
Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 4: Manual smoke check (optional but recommended)**

Run: `wsx config set usage_graph_window 1w` then launch wsx; the footer label should read `1w`. Reset with `wsx config set usage_graph_window 24h`.

- [ ] **Step 5: Commit**

```bash
git add src/app/render.rs
git commit -m "feat(dashboard): render configurable window in footer graph"
```

---

## Task 7: `Modal::UsageWindowPicker` + anchored rendering

**Files:**
- Modify: `src/ui/modal.rs` (enum ~73; guard ~104; new `picker_rect` + `render_usage_window_picker`)
- Modify: `src/app/render.rs` (dispatch the picker render)

- [ ] **Step 1: Write the failing test for `picker_rect`**

Add a test module entry at the bottom of `src/ui/modal.rs` (if a `#[cfg(test)] mod tests` block exists, add to it; otherwise add this block at end of file):

```rust
#[cfg(test)]
mod usage_picker_tests {
    use super::*;
    use ratatui::layout::Rect;

    fn screen() -> Rect {
        Rect { x: 0, y: 0, width: 100, height: 30 }
    }

    #[test]
    fn picker_sits_just_above_anchor_left_aligned() {
        // Anchor = graph on the footer row (y = 29), x = 70.
        let anchor = Rect { x: 70, y: 29, width: 27, height: 1 };
        let r = picker_rect(anchor, screen(), 18, 5);
        assert_eq!(r.x, 70); // left-aligned to anchor
        assert_eq!(r.y, 24); // 29 - 5, directly above
        assert_eq!(r.width, 18);
        assert_eq!(r.height, 5);
    }

    #[test]
    fn picker_clamps_to_right_edge() {
        // Anchor near the right edge would overflow; x clamps so x+w <= width.
        let anchor = Rect { x: 95, y: 29, width: 5, height: 1 };
        let r = picker_rect(anchor, screen(), 18, 5);
        assert_eq!(r.x, 100 - 18);
    }

    #[test]
    fn picker_clamps_to_top_edge() {
        // Anchor high up: y would go negative; clamp to screen top.
        let anchor = Rect { x: 10, y: 2, width: 27, height: 1 };
        let r = picker_rect(anchor, screen(), 18, 5);
        assert_eq!(r.y, 0);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib modal::usage_picker_tests`
Expected: FAIL — `cannot find function picker_rect`.

- [ ] **Step 3: Add the `UsageWindowPicker` modal variant**

In `src/ui/modal.rs`, add to the `Modal` enum (after `AgentsPanel`, before the closing `}` at line ~77):

```rust
    UsageWindowPicker {
        /// Index into `UsageWindow::ALL` for the cursor selection. The current
        /// (applied) window is read separately from the store at render time.
        selected: usize,
    },
```

- [ ] **Step 4: Guard the generic `render()` against the new variant**

In `src/ui/modal.rs`, the `render()` guard (lines ~104-111) early-returns for live-state modals. Add `UsageWindowPicker` to it. Replace:

```rust
    if matches!(
        modal,
        Modal::UpdatesPanel { .. }
            | Modal::ProcessList { .. }
            | Modal::RepoSettings { .. }
            | Modal::AgentsPanel { .. }
    ) {
        return;
    }
```

with:

```rust
    if matches!(
        modal,
        Modal::UpdatesPanel { .. }
            | Modal::ProcessList { .. }
            | Modal::RepoSettings { .. }
            | Modal::AgentsPanel { .. }
            | Modal::UsageWindowPicker { .. }
    ) {
        return;
    }
```

- [ ] **Step 5: Implement `picker_rect` + `render_usage_window_picker`**

First add the `UsageWindow` import at the top of `src/ui/modal.rs` (after line 4, `use crate::ui::theme::Theme;`):

```rust
use crate::config::usage_window::UsageWindow;
```

Then add to `src/ui/modal.rs` near the other render helpers. `Clear`, `Block`, `Borders`, `Paragraph`, `Rect`, `Alignment` are already imported, and `Line`/`Span`/`Style` come in via `ratatui::prelude::*`:

```rust
/// Position a `w`x`h` popup so its bottom edge sits directly above `anchor`,
/// left-aligned to it, clamped to stay fully within `screen`.
fn picker_rect(anchor: Rect, screen: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(screen.width);
    let h = h.min(screen.height);
    let max_x = screen.x + screen.width.saturating_sub(w);
    let x = anchor.x.clamp(screen.x, max_x);
    let y = anchor.y.saturating_sub(h).max(screen.y);
    Rect { x, y, width: w, height: h }
}

/// Render the anchored usage-window picker above the footer graph. Returns the
/// per-option row `Rect`s (in `UsageWindow::ALL` order) for click hit-testing.
pub fn render_usage_window_picker(
    f: &mut Frame,
    screen: Rect,
    selected: usize,
    current: UsageWindow,
    graph_rect: Option<Rect>,
    theme: &Theme,
) -> Vec<Rect> {
    let w: u16 = 18;
    let h: u16 = UsageWindow::ALL.len() as u16 + 2; // options + top/bottom border
    let anchor = graph_rect.unwrap_or(Rect {
        x: screen.x + screen.width.saturating_sub(w),
        y: screen.y + screen.height.saturating_sub(1),
        width: w,
        height: 1,
    });
    let rect = picker_rect(anchor, screen, w, h);
    f.render_widget(Clear, rect);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut rows: Vec<Rect> = Vec::new();
    for (i, win) in UsageWindow::ALL.iter().enumerate() {
        let dot = if *win == current { "•" } else { " " };
        let style = if i == selected {
            theme.selected_bg_style()
        } else {
            theme.header_style()
        };
        lines.push(Line::from(Span::styled(format!(" {dot} {}", win.label()), style)));
        rows.push(Rect {
            x: rect.x + 1,
            y: rect.y + 1 + i as u16,
            width: rect.width.saturating_sub(2),
            height: 1,
        });
    }

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("usage window")
            .title_alignment(Alignment::Left),
    );
    f.render_widget(para, rect);
    rows
}
```

> `selected_bg_style()` and `header_style()` are existing `Theme` methods (`src/ui/theme.rs:240,255`). `Span` comes from `ratatui::prelude::*`, already imported.

- [ ] **Step 6: Dispatch the picker render from `draw()`**

In `src/app/render.rs`, after the existing `if let Some(m) = &app.modal { match m { ... } }` block (the one ending around line 716 with the `other => modal::render(...)` arm), add a separate block that renders the anchored picker and stores its option rects:

```rust
    // The usage-window picker renders anchored over the footer graph rather
    // than centered, so it is handled outside the generic modal dispatch. We
    // copy `selected` out first so the immutable borrow on `app.modal` ends
    // before we assign the returned option rects back to `app`.
    let picker_selected = match &app.modal {
        Some(crate::ui::modal::Modal::UsageWindowPicker { selected }) => Some(*selected),
        _ => None,
    };
    if let Some(selected) = picker_selected {
        let current = crate::config::usage_window::resolve(&app.store);
        let graph_rect = app.usage_graph_rect;
        let area = f.area();
        let rects = crate::ui::modal::render_usage_window_picker(
            f, area, selected, current, graph_rect, &app.theme,
        );
        app.usage_window_option_rects = rects;
    }
```

> `f.area()` is the full screen `Rect` (same call used at the top of `draw()`).

- [ ] **Step 7: Verify build + picker_rect tests pass**

Run: `cargo test --lib modal::usage_picker_tests`
Expected: PASS (3 tests).
Run: `cargo build`
Expected: SUCCESS.

- [ ] **Step 8: Commit**

```bash
git add src/ui/modal.rs src/app/render.rs
git commit -m "feat(modal): anchored usage-window picker rendering"
```

---

## Task 8: Mouse + keyboard interaction for the picker

**Files:**
- Modify: `src/app/input.rs` (`handle_mouse` left-click ~1674; `handle_key_modal` add arm)

- [ ] **Step 1: Open the picker on a graph click; handle option-click and outside-close**

In `src/app/input.rs`, replace the entire `MouseEventKind::Down(MouseButton::Left)` arm (lines ~1674-1705) with the version below. It adds: (a) a leading branch that, when the picker is open, applies a clicked option or dismisses on outside-click; and (b) a trailing branch that opens the picker when the footer graph is clicked and no modal is open.

```rust
        MouseEventKind::Down(MouseButton::Left) => {
            // If the usage-window picker is open, a click either applies the
            // option under the cursor or dismisses the picker (click-outside).
            if matches!(app.modal, Some(Modal::UsageWindowPicker { .. })) {
                if let Some(idx) = app.usage_window_option_rects.iter().position(|r| {
                    m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height)
                }) {
                    let win = crate::config::usage_window::UsageWindow::from_index(idx);
                    let _ = app.store.set_setting("usage_graph_window", win.as_setting());
                }
                app.modal = None;
                return;
            }

            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                fire_chip(app, idx).await;
            } else if let Some((ws_id, _)) = app.attention_rects.iter().copied().find(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking an attention entry attaches to that workspace,
                // identical to `Enter` on the dashboard.
                if let Err(e) = attach_workspace(app, ws_id) {
                    tracing::warn!(error = %e, "failed to attach from attention click");
                }
            } else if let Some((inst, _)) = app.agent_chip_rects.iter().copied().find(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking an agent pill retargets the focused pane to that
                // instance, spawning its session if needed.
                if let Err(e) = app.switch_focused_pane_to(inst) {
                    tracing::warn!(error = %e, "failed to switch pane from agent-pill click");
                }
            } else if app.modal.is_none()
                && app.usage_graph_rect.is_some_and(|r| {
                    m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height)
                })
            {
                // Clicking the footer activity graph opens the window picker,
                // seeded with the currently-applied window.
                let current = crate::config::usage_window::resolve(&app.store);
                app.modal = Some(Modal::UsageWindowPicker {
                    selected: current.index(),
                });
            }
        }
```

> `Modal` is already in scope in this file (used by `handle_key_modal`); if the compiler reports it unresolved here, add `use crate::ui::modal::Modal;` or reference it as `crate::ui::modal::Modal`.

- [ ] **Step 2: Add the keyboard arm in `handle_key_modal`**

In `src/app/input.rs`, inside `handle_key_modal`'s `match modal { ... }`, add a new arm (e.g. after the `Modal::AgentPicker` arm). `UsageWindow::ALL.len()` is 3, so wrap-around uses `2`/`0`:

```rust
        Modal::UsageWindowPicker { selected } => match k.code {
            KeyCode::Up => {
                let n = if selected == 0 {
                    crate::config::usage_window::UsageWindow::ALL.len() - 1
                } else {
                    selected - 1
                };
                app.modal = Some(Modal::UsageWindowPicker { selected: n });
            }
            KeyCode::Down => {
                let n = if selected + 1 >= crate::config::usage_window::UsageWindow::ALL.len() {
                    0
                } else {
                    selected + 1
                };
                app.modal = Some(Modal::UsageWindowPicker { selected: n });
            }
            KeyCode::Enter => {
                let win = crate::config::usage_window::UsageWindow::from_index(selected);
                let _ = app.store.set_setting("usage_graph_window", win.as_setting());
                app.modal = None;
            }
            KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
```

- [ ] **Step 3: Verify the crate builds and all tests pass**

Run: `cargo build`
Expected: SUCCESS.
Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(input): click graph to open usage-window picker; keys to navigate"
```

---

## Task 9: Final verification + lint

**Files:** none (verification only)

- [ ] **Step 1: Full test suite**

Run: `cargo test`
Expected: PASS (all unit + integration tests).

- [ ] **Step 2: Lint clean**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any introduced (e.g. unused imports) and re-run.

- [ ] **Step 3: Manual end-to-end smoke test**

Launch wsx. Verify:
1. Footer shows `24h` + sparkline by default.
2. Click the graph → a small picker appears anchored just above it listing `24h / 1w / 1mo` with `•` on `24h`.
3. ↑↓ moves the highlight; Enter on `1w` closes it and the footer label switches to `1w` with the graph re-bucketed.
4. Re-open, click `1mo` directly → applies and closes.
5. Click outside the open picker → dismisses without change.
6. `wsx config set usage_graph_window 24h` from a shell → on next frame the footer reflects `24h`.

- [ ] **Step 4: Commit (if any lint fixes were needed)**

```bash
git add -A
git commit -m "chore: clippy cleanup for usage-graph window feature"
```

---

## Self-Review Notes

**Spec coverage:**
- Setting + `UsageWindow` enum → Task 1.
- Time-aligned 24-bar aggregation (max semantics, empty→0) → Task 2.
- Dynamic footer label + graph hit-rect → Task 3, Task 6.
- 30-day retention → Task 4.
- Clickable graph + anchored picker (open, navigate, apply, outside-close) → Tasks 5, 7, 8.
- CLI path → no code (existing `wsx config set`), exercised in Task 9 Step 3.
- Error handling (garbage value → Day; DB error → Day) → Task 1 `resolve`/`from_setting`; clamp/off-screen → Task 7 `picker_rect`.

**Type consistency:** `aggregate_buckets(&[(u64,u32)], u64, u64, usize) -> Vec<u32>`, `UsageWindow::{hours,label,as_setting,from_setting,index,from_index,ALL}`, `render_footer(...) -> Rect`, `footer(...) -> (Line, u16)`, `render_usage_window_picker(...) -> Vec<Rect>`, `picker_rect(anchor, screen, w, h) -> Rect`, `Modal::UsageWindowPicker { selected: usize }`, `app.usage_graph_rect: Option<Rect>`, `app.usage_window_option_rects: Vec<Rect>` — names used identically across all tasks.

**Setting key:** `"usage_graph_window"` used identically in `resolve`, the keyboard apply, the mouse apply, and the manual test.
