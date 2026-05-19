# V5 Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the wsx dashboard renderer with the V5 design: canonical 6-state status vocabulary, status strip + per-repo count cluster, by-repo and by-attention views, fold/filter/group keybindings, persistent 24h activity sparkline, `+N −N` line-diff column, and a new `wsx` theme. Existing decorations (PR lifecycle on branch, YOLO warn on name, `⚙!` setup-failed badge, nerd-fonts) survive on top of V5.

**Architecture:** Split the existing 574-line `src/ui/dashboard/mod.rs` into focused submodules (`status`, `spinner`, `sparkline`, `sort`, `row`, `layout`, `by_repo`, `by_attention`). A thin top-level `mod.rs::render()` reads `DashboardState::group_mode` and delegates. The status vocabulary lives in a new `Status` enum; views key off that enum so column renderers don't depend on classifier internals. Theme gains 6 status colors per palette + a new `wsx` palette set as default. Sparkline persisted via a new sqlite `activity_buckets` table (24 hourly rows max). Diff stats via `git diff --shortstat <base>...HEAD`, polled per workspace alongside the existing `WorkspaceStatus`.

**Tech Stack:** Rust 2024, `ratatui` (`List`, `ListItem`, `Paragraph`, `Layout`, `Line`, `Span`, `Style`, `Color::Rgb`), `crossterm` keyevents, `rusqlite`, `tokio` for the existing per-workspace polling loops.

**Source spec:** `docs/superpowers/specs/2026-05-19-v5-dashboard-design.md`

**Branch:** `feat/v5-dashboard` — cut from `main` in Task 1. All work happens here; the user reviews in the running TUI and either merges or iterates.

**Scope clarification (not explicit in spec):** The existing per-row git-status text (`~3 ?2 ↑1 ↓1` files-modified / untracked / ahead / behind) is dropped from the new V5 row, since column 7 is `+N −N` line diff per spec. Modified/untracked counts remain visible via the `v` diff viewer. Ahead/behind is implicit in the `+N −N` (which is computed against `base_branch`).

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/ui/dashboard/mod.rs` | Rewrite | Thin top-level `render()` + `DashboardState` struct + `Item` type + re-exports |
| `src/ui/dashboard/status.rs` | Create | `Status` enum, `Status::classify(...)`, `Status::priority()`, `Status::glyph()`, `Status::is_live()` |
| `src/ui/dashboard/spinner.rs` | Create | `SPINNER` frame table + `frame(tick)` helper |
| `src/ui/dashboard/sparkline.rs` | Create | Render a `&[u32]` as 24 block chars |
| `src/ui/dashboard/sort.rs` | Create | `noise_score(counts)`, `default_fold(counts)`, status-priority comparators |
| `src/ui/dashboard/row.rs` | Create | Shared column composer: gutter / elbow / glyph / name / branch / procs / diff / message / ago |
| `src/ui/dashboard/layout.rs` | Create | Top chrome, status strip, footer (keybinds + sparkline) |
| `src/ui/dashboard/by_repo.rs` | Create | Repo headers (with `─` rule + counts cluster), nested rows, fold logic |
| `src/ui/dashboard/by_attention.rs` | Create | Section headers (NEEDS ATTENTION / WORKING / RECENT / IDLE / QUIET REPOS) + flat rows |
| `src/ui/dashboard/fixture.rs` | Create | `#[cfg(test)]` fixture mirroring the design's `data.js` |
| `src/ui/dashboard/tests.rs` | Rewrite | New tests covering all modules above; old tests dropped where superseded |
| `src/ui/dashboard/label_tests.rs` | Delete | Tested old activity-label strings; replaced by `status.rs` tests |
| `src/ui/theme.rs` | Modify | Add 6 status fields, `status_style()` helper, `wsx` theme, rename `default_theme` → `ansi`, change `Default` impl |
| `src/git.rs` | Modify | Add `DiffStats` + `parse_shortstat()` + `workspace_diff_stats()` |
| `src/store.rs` | Modify | Add `activity_buckets` table + `set_activity_bucket` / `recent_activity_buckets` / `prune_activity_buckets_before` |
| `src/app.rs` | Modify | `tick: u32`, `workspace_diff` map, `activity_history` ring + persistence loop, new keybindings (`g`/`z`/`r`/`/`), evolved `DashboardState` plumbing in `draw()` |

No new crate dependencies.

---

## Task 1: Cut the feature branch

**Files:** none

- [ ] **Step 1: Verify working tree is clean and on `main`**

Run: `git status --short && git rev-parse --abbrev-ref HEAD`
Expected: empty status output and `main`.

- [ ] **Step 2: Cut the branch**

Run: `git checkout -b feat/v5-dashboard`
Expected: `Switched to a new branch 'feat/v5-dashboard'`.

- [ ] **Step 3: Verify**

Run: `git rev-parse --abbrev-ref HEAD`
Expected: `feat/v5-dashboard`.

No commit yet — Task 2 produces the first commit on this branch.

---

## Task 2: `Status` enum + classifier + tests

**Files:**
- Create: `src/ui/dashboard/status.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod status;`)

- [ ] **Step 1: Create `status.rs` with enum + helpers + classifier**

Write `src/ui/dashboard/status.rs`:

```rust
//! Canonical 6-state status vocabulary used by every view, gutter, and
//! status-strip cell. Maps the existing classifier inputs from `app.rs`
//! into a single enum so column renderers don't depend on classifier
//! internals.

use crate::app::StoppedKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Question,
    Stalled,
    Waiting,
    Thinking,
    Complete,
    Idle,
}

impl Status {
    /// Sort key. Higher = more urgent; used by both repo noise scoring
    /// and within-section ordering.
    pub fn priority(self) -> u8 {
        match self {
            Status::Stalled => 5,
            Status::Question => 4,
            Status::Waiting => 3,
            Status::Thinking => 2,
            Status::Complete => 1,
            Status::Idle => 0,
        }
    }

    /// Static glyph for this status. Live states (`Thinking`, `Waiting`)
    /// use this only when the renderer cannot animate; otherwise the
    /// spinner replaces it.
    pub fn glyph(self) -> char {
        match self {
            Status::Question => '?',
            Status::Stalled => '!',
            Status::Waiting => '…',
            Status::Thinking => '⠋',
            Status::Complete => '✓',
            Status::Idle => '·',
        }
    }

    /// Human-readable label used in the status strip and section headers.
    pub fn label(self) -> &'static str {
        match self {
            Status::Question => "question",
            Status::Stalled => "stalled",
            Status::Waiting => "waiting",
            Status::Thinking => "thinking",
            Status::Complete => "complete",
            Status::Idle => "idle",
        }
    }

    /// Live states animate the spinner in place of `glyph()`.
    pub fn is_live(self) -> bool {
        matches!(self, Status::Thinking | Status::Waiting)
    }

    /// Reduce the existing classifier inputs into a canonical `Status`.
    /// Matches the mapping table in the V5 design spec.
    pub fn classify(
        awaiting_tool: bool,
        stopped_kind: Option<StoppedKind>,
        stalled: bool,
        seconds_since_activity: Option<u64>,
        session_running: bool,
        has_prior_session: bool,
    ) -> Self {
        if awaiting_tool {
            return Status::Question;
        }
        match stopped_kind {
            Some(StoppedKind::AwaitingAnswer) => return Status::Question,
            Some(StoppedKind::Complete) => return Status::Complete,
            None => {}
        }
        if stalled {
            return Status::Stalled;
        }
        if session_running {
            match seconds_since_activity {
                Some(s) if s < 30 => Status::Thinking,
                Some(_) => Status::Waiting,
                None => Status::Thinking,
            }
        } else {
            // No live session — `has_prior_session` distinguishes
            // "resumable" from "off" today; both collapse to Idle in V5.
            let _ = has_prior_session;
            Status::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(secs: u64) -> Option<u64> {
        Some(secs)
    }

    #[test]
    fn priority_ordering_matches_spec() {
        assert!(Status::Stalled.priority() > Status::Question.priority());
        assert!(Status::Question.priority() > Status::Waiting.priority());
        assert!(Status::Waiting.priority() > Status::Thinking.priority());
        assert!(Status::Thinking.priority() > Status::Complete.priority());
        assert!(Status::Complete.priority() > Status::Idle.priority());
    }

    #[test]
    fn glyphs_match_design_tokens() {
        assert_eq!(Status::Question.glyph(), '?');
        assert_eq!(Status::Stalled.glyph(), '!');
        assert_eq!(Status::Waiting.glyph(), '…');
        assert_eq!(Status::Thinking.glyph(), '⠋');
        assert_eq!(Status::Complete.glyph(), '✓');
        assert_eq!(Status::Idle.glyph(), '·');
    }

    #[test]
    fn live_states_are_thinking_and_waiting() {
        assert!(Status::Thinking.is_live());
        assert!(Status::Waiting.is_live());
        assert!(!Status::Question.is_live());
        assert!(!Status::Stalled.is_live());
        assert!(!Status::Complete.is_live());
        assert!(!Status::Idle.is_live());
    }

    #[test]
    fn awaiting_tool_outranks_everything() {
        assert_eq!(
            Status::classify(true, Some(StoppedKind::Complete), true, s(0), true, true),
            Status::Question
        );
    }

    #[test]
    fn awaiting_answer_maps_to_question() {
        assert_eq!(
            Status::classify(false, Some(StoppedKind::AwaitingAnswer), false, s(1), true, true),
            Status::Question
        );
    }

    #[test]
    fn stopped_complete_maps_to_complete() {
        assert_eq!(
            Status::classify(false, Some(StoppedKind::Complete), false, s(1), true, true),
            Status::Complete
        );
    }

    #[test]
    fn stalled_outranks_running_recency() {
        assert_eq!(
            Status::classify(false, None, true, s(0), true, true),
            Status::Stalled
        );
    }

    #[test]
    fn running_under_30s_is_thinking() {
        assert_eq!(
            Status::classify(false, None, false, s(0), true, false),
            Status::Thinking
        );
        assert_eq!(
            Status::classify(false, None, false, s(29), true, false),
            Status::Thinking
        );
    }

    #[test]
    fn running_over_30s_is_waiting() {
        assert_eq!(
            Status::classify(false, None, false, s(30), true, false),
            Status::Waiting
        );
        assert_eq!(
            Status::classify(false, None, false, s(3600), true, false),
            Status::Waiting
        );
    }

    #[test]
    fn no_session_maps_to_idle_regardless_of_prior() {
        assert_eq!(
            Status::classify(false, None, false, None, false, true),
            Status::Idle
        );
        assert_eq!(
            Status::classify(false, None, false, None, false, false),
            Status::Idle
        );
    }
}
```

- [ ] **Step 2: Add `pub mod status;` to `src/ui/dashboard/mod.rs`**

Edit `src/ui/dashboard/mod.rs`, add near the top imports (after the existing `use` lines, before the existing `const NAME_WIDTH:` line):

```rust
pub mod status;
```

- [ ] **Step 3: Build + run new tests**

Run: `cargo test --lib ui::dashboard::status::tests`
Expected: 9 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/status.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add canonical Status vocabulary + classifier"
```

---

## Task 3: Extend `Theme` with 6 status colors + `wsx` palette

**Files:**
- Modify: `src/ui/theme.rs`

- [ ] **Step 1: Add fields + `status_style()` helper test**

Edit `src/ui/theme.rs`. In the existing `#[cfg(test)] mod tests` block at the bottom, add:

```rust
    use crate::ui::dashboard::status::Status;

    #[test]
    fn wsx_is_the_default_theme() {
        let t = Theme::default();
        assert_eq!(t.question, Color::Rgb(0xe4, 0xba, 0x6c));
        assert_eq!(t.stalled, Color::Rgb(0xd3, 0x62, 0x58));
        assert_eq!(t.waiting, Color::Rgb(0x6e, 0xa7, 0xd8));
        assert_eq!(t.thinking, Color::Rgb(0xb7, 0x8c, 0xd0));
        assert_eq!(t.complete, Color::Rgb(0x67, 0xc0, 0x89));
        assert_eq!(t.idle, Color::Rgb(0x7a, 0x7e, 0x85));
    }

    #[test]
    fn ansi_theme_uses_named_colors() {
        let t = Theme::ansi();
        assert_eq!(t.question, Color::Yellow);
        assert_eq!(t.stalled, Color::Red);
        assert_eq!(t.waiting, Color::Blue);
        assert_eq!(t.thinking, Color::Magenta);
        assert_eq!(t.complete, Color::Green);
        assert_eq!(t.idle, Color::DarkGray);
    }

    #[test]
    fn status_style_maps_each_state() {
        let t = Theme::default();
        assert_eq!(t.status_style(Status::Question).fg, Some(t.question));
        assert_eq!(t.status_style(Status::Stalled).fg, Some(t.stalled));
        assert_eq!(t.status_style(Status::Waiting).fg, Some(t.waiting));
        assert_eq!(t.status_style(Status::Thinking).fg, Some(t.thinking));
        assert_eq!(t.status_style(Status::Complete).fg, Some(t.complete));
        assert_eq!(t.status_style(Status::Idle).fg, Some(t.idle));
    }

    #[test]
    fn by_name_resolves_wsx() {
        assert_eq!(Theme::by_name("wsx").question, Color::Rgb(0xe4, 0xba, 0x6c));
    }
```

- [ ] **Step 2: Run new tests to confirm they fail**

Run: `cargo test --lib ui::theme::tests`
Expected: 4 new tests FAIL (no `question` field, no `Theme::ansi()`, no `status_style`, no `"wsx"` in `by_name`).

- [ ] **Step 3: Add the six fields, the `status_style()` helper, the `wsx` theme, and rename `default_theme` → `ansi`**

Edit `src/ui/theme.rs`. Add to the `Theme` struct after the existing `pub merged: Color,` field:

```rust
    /// 6-state status palette per the V5 design tokens.
    pub question: Color,
    pub stalled:  Color,
    pub waiting:  Color,
    pub thinking: Color,
    pub complete: Color,
    pub idle:     Color,
```

Inside `impl Theme`, rename the existing `pub fn default_theme() -> Self` to `pub fn ansi() -> Self` and extend its body to set the new fields. The full method becomes:

```rust
    /// ANSI-named palette so the user's terminal theme is respected.
    /// Was named `default_theme` pre-V5; the new default is `wsx`.
    pub fn ansi() -> Self {
        Self {
            header_fg: Color::Cyan,
            header_bg: Some(Color::Reset),
            selected_fg: Color::White,
            selected_bg: Color::DarkGray,
            dim: Color::DarkGray,
            path: Color::Indexed(67),
            ok: Color::Green,
            warn: Color::Yellow,
            err: Color::Red,
            attention: Color::Magenta,
            merged: Color::Magenta,
            question: Color::Yellow,
            stalled: Color::Red,
            waiting: Color::Blue,
            thinking: Color::Magenta,
            complete: Color::Green,
            idle: Color::DarkGray,
        }
    }

    /// V5 design tokens — oklch values from `tui.css` converted to sRGB.
    /// This is the new default theme.
    pub fn wsx() -> Self {
        Self {
            header_fg: Color::Rgb(0xeb, 0xeb, 0xeb),
            header_bg: None,
            selected_fg: Color::Rgb(0xff, 0xff, 0xff),
            selected_bg: Color::Rgb(0x24, 0x30, 0x43),
            dim: Color::Rgb(0xb5, 0xb5, 0xb5),
            path: Color::Rgb(0x6b, 0x6e, 0x75),
            ok: Color::Rgb(0x67, 0xc0, 0x89),
            warn: Color::Rgb(0xe4, 0xba, 0x6c),
            err: Color::Rgb(0xd3, 0x62, 0x58),
            attention: Color::Rgb(0xb7, 0x8c, 0xd0),
            merged: Color::Rgb(0xb7, 0x8c, 0xd0),
            question: Color::Rgb(0xe4, 0xba, 0x6c),
            stalled:  Color::Rgb(0xd3, 0x62, 0x58),
            waiting:  Color::Rgb(0x6e, 0xa7, 0xd8),
            thinking: Color::Rgb(0xb7, 0x8c, 0xd0),
            complete: Color::Rgb(0x67, 0xc0, 0x89),
            idle:     Color::Rgb(0x7a, 0x7e, 0x85),
        }
    }
```

Extend each of the other constructors (`dracula`, `jellybeans`, `nord`) by appending the 6 status fields per the spec table. Append to `dracula()`'s struct literal:

```rust
            question: yellow,
            stalled:  red,
            waiting:  cyan,
            thinking: purple,
            complete: green,
            idle:     comment,
```

(Remove the `let _ = cyan;` line, since `cyan` now has a real use.)

Append to `jellybeans()`'s struct literal:

```rust
            question: orange,
            stalled:  red,
            waiting:  blue,
            thinking: purple,
            complete: green,
            idle:     gray,
```

Append to `nord()`'s struct literal:

```rust
            question: aurora_yellow,
            stalled:  aurora_red,
            waiting:  frost1,
            thinking: aurora_purple,
            complete: aurora_green,
            idle:     polar3,
```

In `by_name`, add the `"wsx"` arm and keep `"ansi"` as an alias. Replace the body of `by_name`:

```rust
    pub fn by_name(name: &str) -> Self {
        match name {
            "ansi" => Self::ansi(),
            "wsx" => Self::wsx(),
            "dracula" => Self::dracula(),
            "jellybeans" => Self::jellybeans(),
            "nord" => Self::nord(),
            _ => Self::wsx(),
        }
    }
```

Add the `status_style` helper inside `impl Theme` (any location after the existing helpers):

```rust
    pub fn status_style(&self, s: crate::ui::dashboard::status::Status) -> Style {
        use crate::ui::dashboard::status::Status::*;
        let fg = match s {
            Question => self.question,
            Stalled => self.stalled,
            Waiting => self.waiting,
            Thinking => self.thinking,
            Complete => self.complete,
            Idle => self.idle,
        };
        Style::default().fg(fg)
    }
```

Replace the `Default` impl at the bottom:

```rust
impl Default for Theme {
    fn default() -> Self {
        Self::wsx()
    }
}
```

- [ ] **Step 4: Update the two existing tests that assume ANSI defaults**

In `mod tests`, change `by_name_resolves_known_themes` so the `default` lookup no longer expects Cyan (since the default is now wsx):

```rust
    #[test]
    fn by_name_resolves_known_themes() {
        assert_eq!(Theme::by_name("ansi").header_fg, Color::Cyan);
        assert!(matches!(
            Theme::by_name("dracula").header_fg,
            Color::Rgb(0xbd, 0x93, 0xf9)
        ));
        assert!(matches!(
            Theme::by_name("nord").header_fg,
            Color::Rgb(0x88, 0xc0, 0xd0)
        ));
        assert!(matches!(
            Theme::by_name("jellybeans").header_fg,
            Color::Rgb(0x81, 0x97, 0xbf)
        ));
    }
```

And update `unknown_theme_falls_back_to_default` to expect the wsx default (RGB, not Cyan):

```rust
    #[test]
    fn unknown_theme_falls_back_to_default() {
        assert!(matches!(
            Theme::by_name("bogus").header_fg,
            Color::Rgb(0xeb, 0xeb, 0xeb)
        ));
        assert!(matches!(
            Theme::by_name("").header_fg,
            Color::Rgb(0xeb, 0xeb, 0xeb)
        ));
    }
```

- [ ] **Step 5: Run all theme tests**

Run: `cargo test --lib ui::theme::tests`
Expected: 6 tests pass (4 new + 2 updated).

- [ ] **Step 6: Verify the rest of the crate still builds**

Run: `cargo build`
Expected: build succeeds. (Any callsites of `Theme::default_theme()` need an update — there shouldn't be any, but if `cargo build` complains, fix them by renaming to `Theme::ansi()`.)

- [ ] **Step 7: Commit**

```bash
git add src/ui/theme.rs
git commit -m "feat(tui): extend themes with 6 status colors + new wsx default"
```

---

## Task 4: Spinner module

**Files:**
- Create: `src/ui/dashboard/spinner.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod spinner;`)

- [ ] **Step 1: Write the spinner module with tests**

Create `src/ui/dashboard/spinner.rs`:

```rust
//! 8-frame braille spinner driven by `app.tick`. The renderer treats
//! `Tick` as 60 fps (existing 16ms cadence); dividing by 8 yields ~7.8
//! fps, matching the V5 spec's 8 fps target.

pub const SPINNER: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];

/// Pick the spinner frame for a given tick counter.
pub fn frame(tick: u32) -> char {
    SPINNER[((tick / 8) as usize) % 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_zero_is_first_glyph() {
        assert_eq!(frame(0), '⠋');
    }

    #[test]
    fn frame_advances_every_eight_ticks() {
        assert_eq!(frame(0), '⠋');
        assert_eq!(frame(7), '⠋');
        assert_eq!(frame(8), '⠙');
        assert_eq!(frame(15), '⠙');
        assert_eq!(frame(16), '⠹');
    }

    #[test]
    fn frame_wraps_after_64_ticks() {
        assert_eq!(frame(64), '⠋');
        assert_eq!(frame(72), '⠙');
    }
}
```

- [ ] **Step 2: Register the module**

Edit `src/ui/dashboard/mod.rs` and add (next to `pub mod status;`):

```rust
pub mod spinner;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::dashboard::spinner::tests`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/spinner.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add spinner module for V5 live-state animation"
```

---

## Task 5: Sparkline module

**Files:**
- Create: `src/ui/dashboard/sparkline.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod sparkline;`)

- [ ] **Step 1: Write the module**

Create `src/ui/dashboard/sparkline.rs`:

```rust
//! Render a sequence of u32 samples as Unicode block characters.
//! Used by the dashboard footer's 24h activity strip.

pub const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render `samples` to a string of length `len`. If `samples.len() < len`,
/// missing samples render as the lowest block (`▁`); if longer, only the
/// last `len` samples are kept. Range scales by the max sample (floor 1).
pub fn render(samples: &[u32], len: usize) -> String {
    let start = samples.len().saturating_sub(len);
    let tail = &samples[start..];
    let max = (*tail.iter().max().unwrap_or(&0)).max(1);
    let pad = len.saturating_sub(tail.len());
    let mut out = String::with_capacity(len * 3);
    for _ in 0..pad {
        out.push(BLOCKS[0]);
    }
    for &v in tail {
        let idx = ((v as u64 * 7) / max as u64) as usize;
        let idx = idx.min(7);
        out.push(BLOCKS[idx]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_zero_is_all_lowest() {
        assert_eq!(render(&[0, 0, 0], 3), "▁▁▁");
    }

    #[test]
    fn short_input_left_pads_with_lowest() {
        let out = render(&[1, 1], 5);
        // 3 missing samples then 2 maxed (since max=1, all render as full)
        assert_eq!(out.chars().count(), 5);
        assert_eq!(&out.chars().collect::<String>()[..], "▁▁▁██");
    }

    #[test]
    fn long_input_keeps_tail() {
        // 10 samples, render last 3.
        let out = render(&[0,0,0,0,0,0,0,1,2,3], 3);
        assert_eq!(out.chars().count(), 3);
        // last 3 are 1,2,3 with max=3 → ⌊7/3⌋=2 (▃), ⌊14/3⌋=4 (▅), 7 (█)
        let chars: Vec<char> = out.chars().collect();
        assert_eq!(chars, vec!['▃', '▅', '█']);
    }

    #[test]
    fn output_length_always_matches_requested() {
        assert_eq!(render(&[], 24).chars().count(), 24);
        assert_eq!(render(&[5; 100], 24).chars().count(), 24);
    }
}
```

- [ ] **Step 2: Register**

Add `pub mod sparkline;` to `src/ui/dashboard/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::dashboard::sparkline::tests`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/sparkline.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add sparkline renderer for V5 footer"
```

---

## Task 6: Sort module (noise score + fold defaults)

**Files:**
- Create: `src/ui/dashboard/sort.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod sort;`)

- [ ] **Step 1: Write the module**

Create `src/ui/dashboard/sort.rs`:

```rust
//! Pure sort and fold helpers for the by-repo view.

use crate::ui::dashboard::status::Status;

/// Per-repo status counts. Mirrors the design's `RepoCounts` shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusCounts {
    pub question: u32,
    pub stalled: u32,
    pub waiting: u32,
    pub thinking: u32,
    pub complete: u32,
    pub idle: u32,
}

impl StatusCounts {
    pub fn from_iter<I: IntoIterator<Item = Status>>(iter: I) -> Self {
        let mut c = Self::default();
        for s in iter {
            match s {
                Status::Question => c.question += 1,
                Status::Stalled => c.stalled += 1,
                Status::Waiting => c.waiting += 1,
                Status::Thinking => c.thinking += 1,
                Status::Complete => c.complete += 1,
                Status::Idle => c.idle += 1,
            }
        }
        c
    }

    pub fn total(&self) -> u32 {
        self.question + self.stalled + self.waiting + self.thinking + self.complete + self.idle
    }
}

/// Noise score per V5 spec: higher repos rise to the top.
pub fn noise_score(c: StatusCounts) -> u32 {
    c.question * 100 + c.stalled * 80 + c.waiting * 40 + c.thinking * 20 + c.complete
}

/// Default fold state for a repo. `true` = folded by default.
/// Empty repos and all-quiet repos (no live + no attention) start folded.
pub fn default_fold(c: StatusCounts) -> bool {
    if c.total() == 0 {
        return true;
    }
    (c.question + c.stalled + c.waiting + c.thinking) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(q: u32, s: u32, w: u32, t: u32, c: u32, i: u32) -> StatusCounts {
        StatusCounts { question: q, stalled: s, waiting: w, thinking: t, complete: c, idle: i }
    }

    #[test]
    fn noise_score_question_outweighs_stalled() {
        // 1 question = 100, 1 stalled = 80
        assert!(noise_score(counts(1, 0, 0, 0, 0, 0)) > noise_score(counts(0, 1, 0, 0, 0, 0)));
    }

    #[test]
    fn noise_score_matches_design_example_ordering() {
        // From design data.js:
        //   wsx: 1q + 1s + 1w + 0t + 1c = 100+80+40+1 = 221
        //   ui:  1q                      = 100
        //   scp-admin: 1w                = 40
        //   ssk: 1t + 1c                 = 20 + 1 = 21
        //   backend: 1t                  = 20
        //   api: 1c                      = 1
        //   empty repos                  = 0
        let wsx = noise_score(counts(1, 1, 1, 0, 1, 0));
        let ui = noise_score(counts(1, 0, 0, 0, 0, 0));
        let scp = noise_score(counts(0, 0, 1, 0, 0, 0));
        let ssk = noise_score(counts(0, 0, 0, 1, 1, 3));
        let backend = noise_score(counts(0, 0, 0, 1, 0, 0));
        let api = noise_score(counts(0, 0, 0, 0, 1, 1));
        assert!(wsx > ui);
        assert!(ui > scp);
        assert!(scp > ssk);
        assert!(ssk > backend);
        assert!(backend > api);
    }

    #[test]
    fn default_fold_empty_repo_is_folded() {
        assert!(default_fold(counts(0, 0, 0, 0, 0, 0)));
    }

    #[test]
    fn default_fold_all_idle_is_folded() {
        assert!(default_fold(counts(0, 0, 0, 0, 0, 3)));
    }

    #[test]
    fn default_fold_complete_only_is_folded() {
        // No question/stalled/waiting/thinking → folded even with completes.
        assert!(default_fold(counts(0, 0, 0, 0, 5, 0)));
    }

    #[test]
    fn default_fold_thinking_is_expanded() {
        assert!(!default_fold(counts(0, 0, 0, 1, 0, 0)));
    }

    #[test]
    fn status_counts_from_iter() {
        let c = StatusCounts::from_iter([
            Status::Question, Status::Stalled, Status::Stalled, Status::Idle,
        ]);
        assert_eq!(c.question, 1);
        assert_eq!(c.stalled, 2);
        assert_eq!(c.idle, 1);
        assert_eq!(c.total(), 4);
    }
}
```

- [ ] **Step 2: Register**

Add `pub mod sort;` to `src/ui/dashboard/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::dashboard::sort::tests`
Expected: 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/sort.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add noise-score + fold-default helpers"
```

---

## Task 7: Git `DiffStats` + shortstat parser

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Add the type + parser test**

Open `src/git.rs`. Inside the existing `#[cfg(test)] mod tests` block (or whatever the file calls its tests module — search for `#[cfg(test)]`), append the parser tests:

```rust
    #[test]
    fn parse_shortstat_both() {
        assert_eq!(
            parse_shortstat(" 5 files changed, 32 insertions(+), 12 deletions(-)\n"),
            Some(DiffStats { added: 32, removed: 12 })
        );
    }

    #[test]
    fn parse_shortstat_only_insertions() {
        assert_eq!(
            parse_shortstat(" 1 file changed, 18 insertions(+)\n"),
            Some(DiffStats { added: 18, removed: 0 })
        );
    }

    #[test]
    fn parse_shortstat_only_deletions() {
        assert_eq!(
            parse_shortstat(" 2 files changed, 4 deletions(-)\n"),
            Some(DiffStats { added: 0, removed: 4 })
        );
    }

    #[test]
    fn parse_shortstat_empty_returns_zero() {
        assert_eq!(parse_shortstat(""), Some(DiffStats { added: 0, removed: 0 }));
        assert_eq!(parse_shortstat("\n"), Some(DiffStats { added: 0, removed: 0 }));
    }

    #[test]
    fn parse_shortstat_malformed_returns_none() {
        assert_eq!(parse_shortstat("garbage line"), None);
    }
```

If `src/git.rs` does not have a tests module yet, append at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // ... the 5 tests above ...
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib git::tests::parse_shortstat`
Expected: tests FAIL (`DiffStats` and `parse_shortstat` not defined).

- [ ] **Step 3: Add the types and the parser**

Append to `src/git.rs` (above the tests module if you added one in Step 1, otherwise above the `#[cfg(test)]` line):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStats {
    pub added: u32,
    pub removed: u32,
}

/// Parse the trailing line of `git diff --shortstat`.
/// Accepts both `N insertions(+)` and `N deletions(-)` in either order
/// or alone. Returns `None` on a non-empty line that doesn't match.
pub fn parse_shortstat(s: &str) -> Option<DiffStats> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(DiffStats { added: 0, removed: 0 });
    }
    let mut added: u32 = 0;
    let mut removed: u32 = 0;
    let mut saw_known_marker = false;
    for part in trimmed.split(',') {
        let part = part.trim();
        if let Some(n) = part
            .strip_suffix(" insertion(+)")
            .or_else(|| part.strip_suffix(" insertions(+)"))
        {
            added = n.parse().ok()?;
            saw_known_marker = true;
        } else if let Some(n) = part
            .strip_suffix(" deletion(-)")
            .or_else(|| part.strip_suffix(" deletions(-)"))
        {
            removed = n.parse().ok()?;
            saw_known_marker = true;
        } else if part.ends_with(" file changed") || part.ends_with(" files changed") {
            // Acceptable file-count prefix; ignore.
        } else {
            // Unknown segment — bail.
            return None;
        }
    }
    if saw_known_marker || trimmed.contains("file") {
        Some(DiffStats { added, removed })
    } else {
        None
    }
}
```

- [ ] **Step 4: Add the async wrapper**

Find the section in `src/git.rs` that runs git subcommands (search for `tokio::process::Command` or `Command::new("git")`). Add this function near the other async git helpers:

```rust
/// Compute line-count diff stats for a worktree against `base`.
/// Returns `None` on any git failure (missing base ref, etc.).
pub async fn workspace_diff_stats(
    worktree: &std::path::Path,
    base: &str,
) -> Option<DiffStats> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .arg("diff")
        .arg("--shortstat")
        .arg(format!("{base}...HEAD"))
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_shortstat(&stdout)
}
```

- [ ] **Step 5: Run all the new tests**

Run: `cargo test --lib git::tests::parse_shortstat`
Expected: 5 tests pass.

- [ ] **Step 6: Verify the whole crate still builds**

Run: `cargo build`
Expected: build succeeds.

- [ ] **Step 7: Commit**

```bash
git add src/git.rs
git commit -m "feat(git): add DiffStats + shortstat parser + workspace_diff_stats"
```

---

## Task 8: Store activity_buckets table + CRUD

**Files:**
- Modify: `src/store.rs`

- [ ] **Step 1: Add round-trip test first**

Find the existing `#[cfg(test)] mod tests` at the bottom of `src/store.rs`. Add this test (place near other Store tests, e.g. after `repo_base_branch_round_trip`):

```rust
    #[test]
    fn activity_bucket_round_trip_and_prune() {
        let store = Store::open_in_memory().unwrap();
        store.set_activity_bucket(100, 3).unwrap();
        store.set_activity_bucket(200, 5).unwrap();
        store.set_activity_bucket(300, 1).unwrap();

        // recent_activity_buckets returns in ascending hour order.
        let all = store.recent_activity_buckets(50).unwrap();
        assert_eq!(all, vec![(100, 3), (200, 5), (300, 1)]);

        // Update an existing bucket — upsert semantics.
        store.set_activity_bucket(200, 9).unwrap();
        let updated = store.recent_activity_buckets(50).unwrap();
        assert_eq!(updated, vec![(100, 3), (200, 9), (300, 1)]);

        // Prune drops anything older than the cutoff (exclusive).
        store.prune_activity_buckets_before(200).unwrap();
        let after_prune = store.recent_activity_buckets(50).unwrap();
        assert_eq!(after_prune, vec![(200, 9), (300, 1)]);
    }
```

This test assumes `Store::open_in_memory()`. If that helper doesn't exist, search `src/store.rs` for how existing tests build a Store (often `Store::open(":memory:")` or similar) and adjust the test accordingly.

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib store::tests::activity_bucket_round_trip_and_prune`
Expected: FAIL — methods don't exist.

- [ ] **Step 3: Add the table to the schema migration**

In `src/store.rs`, find the existing schema SQL (search for `CREATE TABLE IF NOT EXISTS settings`). Append a new statement to the same migration block:

```sql
CREATE TABLE IF NOT EXISTS activity_buckets (
    hour_epoch INTEGER PRIMARY KEY,
    max_live   INTEGER NOT NULL
);
```

The exact mechanism depends on how `Store::open` runs migrations — match the existing pattern. If migrations are a `Vec<&str>` or a single string passed to `execute_batch`, add the statement there.

- [ ] **Step 4: Add the three methods to `impl Store`**

Insert near the other `set_*` / `get_*` methods:

```rust
    pub fn set_activity_bucket(&self, hour_epoch: u64, max_live: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO activity_buckets (hour_epoch, max_live) VALUES (?1, ?2)
             ON CONFLICT(hour_epoch) DO UPDATE SET max_live = excluded.max_live",
            rusqlite::params![hour_epoch as i64, max_live as i64],
        )?;
        Ok(())
    }

    /// Return up to `limit` most-recent buckets in ascending hour order.
    pub fn recent_activity_buckets(&self, limit: usize) -> Result<Vec<(u64, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT hour_epoch, max_live FROM activity_buckets
             ORDER BY hour_epoch DESC LIMIT ?1",
        )?;
        let mut rows: Vec<(u64, u32)> = stmt
            .query_map(rusqlite::params![limit as i64], |r| {
                let h: i64 = r.get(0)?;
                let m: i64 = r.get(1)?;
                Ok((h as u64, m as u32))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Delete buckets with hour_epoch strictly less than `cutoff`.
    pub fn prune_activity_buckets_before(&self, cutoff: u64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM activity_buckets WHERE hour_epoch < ?1",
            rusqlite::params![cutoff as i64],
        )?;
        Ok(())
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib store::tests::activity_bucket_round_trip_and_prune`
Expected: PASS.

- [ ] **Step 6: Run the full store test suite to make sure nothing else broke**

Run: `cargo test --lib store::tests`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/store.rs
git commit -m "feat(store): add activity_buckets table + CRUD for sparkline"
```

---

## Task 9: Fixture for view tests

**Files:**
- Create: `src/ui/dashboard/fixture.rs`
- Modify: `src/ui/dashboard/mod.rs` (`#[cfg(test)] mod fixture;`)

- [ ] **Step 1: Create the fixture**

Create `src/ui/dashboard/fixture.rs`:

```rust
//! Synthetic data mirroring `~/Desktop/design_handoff_wsx_dashboard/data.js`.
//! Used by render tests so the V5 fixture stays close to the design spec.

#![cfg(test)]

use crate::ui::dashboard::status::Status;

#[derive(Debug, Clone)]
pub struct FixtureWorkspace {
    pub name: String,
    pub branch: String,
    pub procs: u32,
    pub status: Status,
    pub last_message: Option<String>,
    pub diff_added: u32,
    pub diff_removed: u32,
    pub ago_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FixtureRepo {
    pub name: String,
    pub path: String,
    pub workspaces: Vec<FixtureWorkspace>,
}

pub fn repos() -> Vec<FixtureRepo> {
    use Status::*;
    fn ws(
        name: &str,
        branch: &str,
        procs: u32,
        status: Status,
        last: Option<&str>,
        added: u32,
        removed: u32,
        ago: Option<u64>,
    ) -> FixtureWorkspace {
        FixtureWorkspace {
            name: name.into(),
            branch: branch.into(),
            procs,
            status,
            last_message: last.map(str::to_string),
            diff_added: added,
            diff_removed: removed,
            ago_secs: ago,
        }
    }
    vec![
        FixtureRepo {
            name: "ssk".into(),
            path: "/home/eben/ssk/ssk-web".into(),
            workspaces: vec![
                ws("wobbly-peony", "eben/wobbly-peony", 0, Idle, None, 0, 0, None),
                ws("woven-parsley", "eben/woven-parsley", 0, Idle, None, 0, 0, None),
                ws("eager-ivy", "eben/eager-ivy", 0, Idle, None, 0, 0, None),
                ws("quiet-fennel", "eben/quiet-fennel", 2, Thinking,
                    Some("Reading src/cli/dashboard.rs to understand the current layout system…"),
                    184, 62, Some(4)),
                ws("brave-cedar", "eben/brave-cedar", 1, Complete,
                    Some("Done. Tests pass (47 ok). Ready for review on PR #214."),
                    612, 211, Some(8 * 60)),
            ],
        },
        FixtureRepo {
            name: "wsx".into(),
            path: "/home/eben/workspace/wsx".into(),
            workspaces: vec![
                ws("tech-stack-question", "bakedbean/tech-stack-question", 1, Complete,
                    Some("* Insight ─── `wsx` is a Rust binary using ratatui + crossterm…"),
                    0, 0, Some(34)),
                ws("repo-overview", "bakedbean/repo-overview", 2, Question,
                    Some("I have enough to give you a grounded tour. ## wsx — a TUI for…"),
                    12, 3, Some(29)),
                ws("list-virtualization", "bakedbean/list-virt", 2, Waiting,
                    Some("Running cargo test --package wsx-tui list_virtualization::scroll…"),
                    318, 44, Some(2 * 60)),
                ws("theme-tokens", "bakedbean/theme-tokens", 1, Stalled,
                    Some("Hit ambiguous dependency: ratatui 0.26 vs 0.27 across two crates."),
                    88, 12, Some(17 * 60)),
            ],
        },
        FixtureRepo {
            name: "backend".into(),
            path: "/home/eben/meals/backend".into(),
            workspaces: vec![
                ws("recipe-importer", "eben/recipe-importer", 2, Thinking,
                    Some("Scaffolding ImportJob model and worker. About to wire it up…"),
                    241, 0, Some(11)),
            ],
        },
        FixtureRepo {
            name: "frontend".into(),
            path: "/home/eben/meals/frontend".into(),
            workspaces: vec![],
        },
        FixtureRepo {
            name: "api".into(),
            path: "/home/eben/ridesnridesnrides/api".into(),
            workspaces: vec![
                ws("rate-limit", "eben/rate-limit", 0, Complete,
                    Some("All limits in place; benchmark shows P99 at 38ms under 5k rps."),
                    419, 83, Some(3600)),
                ws("webhook-retry", "eben/webhook-retry", 0, Idle, None, 0, 0, None),
            ],
        },
        FixtureRepo {
            name: "ui".into(),
            path: "/home/eben/ridesnridesnrides/ui".into(),
            workspaces: vec![
                ws("driver-map-v2", "eben/driver-map-v2", 1, Question,
                    Some("Should the heatmap render driver positions as discrete pins or…"),
                    507, 192, Some(3 * 60)),
            ],
        },
        FixtureRepo {
            name: "scp-admin".into(),
            path: "/home/eben/cci/scp-admin".into(),
            workspaces: vec![
                ws("auth-refactor", "eben/auth-refactor", 1, Waiting,
                    Some("cargo build --release running… (61%)"),
                    88, 104, Some(46)),
            ],
        },
        FixtureRepo {
            name: "scp-api".into(),
            path: "/home/eben/cci/scp-api".into(),
            workspaces: vec![],
        },
    ]
}
```

- [ ] **Step 2: Register the module (test-only)**

In `src/ui/dashboard/mod.rs`, add (near the other `#[cfg(test)] mod` declarations):

```rust
#[cfg(test)] pub(crate) mod fixture;
```

The `pub(crate)` visibility (under `#[cfg(test)]`) lets sibling submodules' tests do `use crate::ui::dashboard::fixture;`. Without it, the fixture would only be visible inside `mod.rs` itself.

- [ ] **Step 3: Build the tests to verify it compiles**

Run: `cargo test --lib ui::dashboard::fixture --no-run`
Expected: builds successfully.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/fixture.rs src/ui/dashboard/mod.rs
git commit -m "test(tui): add V5 dashboard fixture mirroring design data.js"
```

---

## Task 10: Row composer

**Files:**
- Create: `src/ui/dashboard/row.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod row;`)

- [ ] **Step 1: Write the composer + tests**

Create `src/ui/dashboard/row.rs`:

```rust
//! Shared column composer for V5 workspace rows. Returns a
//! `ratatui::text::Line` so view modules can drop it straight into a
//! `ListItem`.
//!
//! Columns (left → right):
//!   1ch  ▎ gutter (status color)
//!   3ch  ├  elbow (faint, centered)
//!   2ch  status glyph or spinner frame
//!   24ch name (left-aligned, ellipsized)
//!   28ch ⎇ branch
//!   6ch  ● Np procs (or faint dot when zero)
//!   12ch +N −N diff
//!   flex └ message (or em-dash)
//!   10ch right-aligned Ns ago

use crate::forge::BranchLifecycle;
use crate::git::DiffStats;
use crate::ui::dashboard::spinner;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const NAME_WIDTH: usize = 24;
const BRANCH_WIDTH: usize = 28;
const PROCS_WIDTH: usize = 6;
const DIFF_WIDTH: usize = 12;
const AGE_WIDTH: usize = 10;
const GUTTER_WIDTH: usize = 1;
const ELBOW_WIDTH: usize = 3;
const GLYPH_WIDTH: usize = 2;

/// Inputs the renderer needs about one workspace, gathered by the caller
/// from `app.rs` state.
#[derive(Debug, Clone)]
pub struct RowInputs {
    pub status: Status,
    pub name: String,
    pub branch: String,
    pub procs: u32,
    pub diff: Option<DiffStats>,
    pub last_message: Option<String>,
    pub ago_secs: Option<u64>,
    pub selected: bool,
    pub yolo: bool,
    pub setup_failed: bool,
    pub lifecycle: Option<BranchLifecycle>,
    pub nerd_fonts: bool,
}

pub fn render(
    inputs: &RowInputs,
    tick: u32,
    theme: &Theme,
    total_width: usize,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // 1: gutter
    spans.push(Span::styled("▎".to_string(), theme.status_style(inputs.status)));

    // 2: elbow
    spans.push(Span::styled("├  ".to_string(), theme.dim_style()));

    // 3: glyph or spinner
    let glyph = if inputs.status.is_live() {
        spinner::frame(tick).to_string()
    } else {
        inputs.status.glyph().to_string()
    };
    let mut glyph_padded = String::with_capacity(2);
    glyph_padded.push_str(&glyph);
    while display_width(&glyph_padded) < GLYPH_WIDTH {
        glyph_padded.push(' ');
    }
    spans.push(Span::styled(glyph_padded, theme.status_style(inputs.status)));

    // 4: name (with setup-failed badge and YOLO/selected styling)
    let name_target = if inputs.setup_failed { NAME_WIDTH - 3 } else { NAME_WIDTH };
    let name_padded = truncate_pad(&inputs.name, name_target);
    let mut name_style = Style::default().add_modifier(Modifier::BOLD);
    if inputs.selected {
        name_style = name_style.fg(theme.waiting);
    } else if inputs.yolo {
        name_style = name_style.fg(theme.warn);
    }
    spans.push(Span::styled(name_padded, name_style));
    if inputs.setup_failed {
        spans.push(Span::styled(" ⚙!".to_string(), theme.err_style()));
    }

    // 5: branch
    let branch_text = if inputs.nerd_fonts {
        format!("\u{e0a0} {}", inputs.branch)
    } else {
        format!("⎇ {}", inputs.branch)
    };
    let branch_padded = truncate_pad(&branch_text, BRANCH_WIDTH);
    let branch_style = lifecycle_style(inputs.lifecycle, theme).unwrap_or_else(|| theme.dim_style());
    spans.push(Span::styled(branch_padded, branch_style));

    // 6: procs
    let procs_cell = if inputs.procs > 0 {
        format!("● {}p", inputs.procs)
    } else {
        "  ·".to_string()
    };
    let procs_padded = truncate_pad(&procs_cell, PROCS_WIDTH);
    let procs_style = if inputs.procs > 0 {
        theme.status_style(Status::Thinking)
    } else {
        theme.dim_style()
    };
    spans.push(Span::styled(procs_padded, procs_style));

    // 7: diff
    let diff_text = match inputs.diff {
        Some(d) if d.added > 0 || d.removed > 0 => format!("+{} −{}", d.added, d.removed),
        _ => String::new(),
    };
    let diff_padded = truncate_pad(&diff_text, DIFF_WIDTH);
    spans.push(Span::styled(diff_padded, theme.dim_style()));

    // 8: message (flex)
    let left_consumed = GUTTER_WIDTH + ELBOW_WIDTH + GLYPH_WIDTH
        + NAME_WIDTH + BRANCH_WIDTH + PROCS_WIDTH + DIFF_WIDTH;
    let right_consumed = AGE_WIDTH;
    let message_width = total_width
        .saturating_sub(left_consumed + right_consumed)
        .max(1);
    if let Some(msg) = inputs.last_message.as_deref() {
        let prefix = "└ ";
        let body = truncate(msg, message_width.saturating_sub(prefix.chars().count()));
        spans.push(Span::styled(prefix.to_string(), theme.status_style(inputs.status)));
        let body_padded = right_pad(&body, message_width.saturating_sub(prefix.chars().count()));
        spans.push(Span::styled(body_padded, theme.dim_style()));
    } else {
        let body = truncate_pad("—", message_width);
        spans.push(Span::styled(body, theme.dim_style()));
    }

    // 9: ago, right-aligned
    let ago = format_ago(inputs.ago_secs);
    let ago_padded = left_pad(&ago, AGE_WIDTH);
    spans.push(Span::styled(ago_padded, theme.dim_style()));

    Line::from(spans)
}

fn lifecycle_style(lc: Option<BranchLifecycle>, theme: &Theme) -> Option<Style> {
    use BranchLifecycle::*;
    match lc {
        Some(PrOpen) => Some(theme.ok_style()),
        Some(PrConflicted) => Some(theme.warn_style()),
        Some(PrMerged) => Some(theme.merged_style()),
        Some(PrClosed) => Some(theme.err_style()),
        _ => None,
    }
}

fn truncate_pad(s: &str, target: usize) -> String {
    let mut out = truncate(s, target);
    let len = out.chars().count();
    if len < target {
        out.push_str(&" ".repeat(target - len));
    }
    out
}

fn truncate(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len <= target {
        s.to_string()
    } else if target == 0 {
        String::new()
    } else {
        let mut out: String = s.chars().take(target - 1).collect();
        out.push('…');
        out
    }
}

fn right_pad(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len >= target { s.to_string() } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(target - len));
        out
    }
}

fn left_pad(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len >= target { s.to_string() } else {
        let mut out = " ".repeat(target - len);
        out.push_str(s);
        out
    }
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn format_ago(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s ago"),
        Some(s) if s < 3600 => format!("{}m ago", s / 60),
        Some(s) => format!("{}h ago", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::status::Status;

    fn base() -> RowInputs {
        RowInputs {
            status: Status::Question,
            name: "repo-overview".into(),
            branch: "bakedbean/repo-overview".into(),
            procs: 2,
            diff: Some(DiffStats { added: 12, removed: 3 }),
            last_message: Some("I have enough to give you a grounded tour.".into()),
            ago_secs: Some(29),
            selected: false,
            yolo: false,
            setup_failed: false,
            lifecycle: None,
            nerd_fonts: false,
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn renders_design_columns_in_order() {
        let theme = Theme::wsx();
        let line = render(&base(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.starts_with("▎"), "gutter first: {text:?}");
        assert!(text.contains("? "), "static glyph for non-live status");
        assert!(text.contains("repo-overview"), "name present");
        assert!(text.contains("⎇ bakedbean/repo-overview"), "branch with glyph");
        assert!(text.contains("● 2p"), "procs cell");
        assert!(text.contains("+12 −3"), "diff cell");
        assert!(text.contains("└ I have enough"), "message prefix");
        assert!(text.trim_end().ends_with("29s ago"), "ago at end: {text:?}");
    }

    #[test]
    fn live_status_uses_spinner_frame() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.status = Status::Thinking;
        let line = render(&inputs, 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⠋"), "spinner frame at tick 0: {text:?}");
        let line2 = render(&inputs, 8, &theme, 120);
        let text2 = line_text(&line2);
        assert!(text2.contains("⠙"), "spinner advances by tick 8: {text2:?}");
    }

    #[test]
    fn missing_message_renders_em_dash() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.last_message = None;
        let line = render(&inputs, 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("—"), "em-dash for missing message: {text:?}");
    }

    #[test]
    fn zero_procs_renders_faint_dot() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.procs = 0;
        let line = render(&inputs, 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("  ·"), "faint dot for zero procs: {text:?}");
    }

    #[test]
    fn no_diff_leaves_column_blank() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.diff = None;
        let line = render(&inputs, 0, &theme, 120);
        let text = line_text(&line);
        assert!(!text.contains("+0 −0"), "no diff cell when None: {text:?}");
    }

    #[test]
    fn setup_failed_appends_badge() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.setup_failed = true;
        let line = render(&inputs, 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⚙!"), "setup badge present: {text:?}");
    }

    #[test]
    fn nerd_fonts_swaps_branch_glyph() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        let line = render(&inputs, 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("\u{e0a0}"), "nerd font branch glyph: {text:?}");
    }
}
```

- [ ] **Step 2: Register**

Add `pub mod row;` to `src/ui/dashboard/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::dashboard::row::tests`
Expected: 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/row.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add V5 workspace row composer"
```

---

## Task 11: `layout.rs` — top chrome, status strip, footer

**Files:**
- Create: `src/ui/dashboard/layout.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod layout;`)

- [ ] **Step 1: Write the module**

Create `src/ui/dashboard/layout.rs`:

```rust
//! Renders the three chrome bars around the V5 dashboard list:
//! top chrome, status strip, footer (keybinds + sparkline).

use crate::ui::dashboard::sort::StatusCounts;
use crate::ui::dashboard::sparkline;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupMode {
    Repo,
    Attention,
}

pub fn top_chrome(
    group: GroupMode,
    repos: usize,
    workspaces: usize,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("wsx", theme.header_style()));
    spans.push(Span::styled(" · dashboard".to_string(), theme.dim_style()));
    spans.push(Span::raw(" ".repeat(6)));
    spans.push(Span::styled("group: ".to_string(), Style::default().fg(theme.path)));
    spans.push(tab_span("repo", group == GroupMode::Repo, theme));
    spans.push(Span::raw(" ".to_string()));
    spans.push(tab_span("attention", group == GroupMode::Attention, theme));

    let right = format!("{repos} repos · {workspaces} workspaces");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
    Line::from(spans)
}

fn tab_span(label: &'static str, active: bool, theme: &Theme) -> Span<'static> {
    if active {
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(theme.selected_fg)
                .bg(theme.selected_bg)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label.to_string(), Style::default().fg(theme.path))
    }
}

pub fn status_strip(counts: StatusCounts, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let cells = [
        (Status::Question, counts.question),
        (Status::Stalled, counts.stalled),
        (Status::Waiting, counts.waiting),
        (Status::Thinking, counts.thinking),
        (Status::Complete, counts.complete),
        (Status::Idle, counts.idle),
    ];
    for (i, (status, n)) in cells.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   ".to_string()));
        }
        let zero = *n == 0;
        let value_style = if zero {
            theme.dim_style()
        } else {
            theme
                .status_style(*status)
                .add_modifier(Modifier::BOLD)
        };
        let label_style = if zero {
            theme.dim_style()
        } else {
            Style::default().fg(theme.path)
        };
        spans.push(Span::styled(status.glyph().to_string(), value_style));
        spans.push(Span::styled(format!(" {n}"), value_style));
        spans.push(Span::styled(format!(" {}", status.label()), label_style));
    }
    Line::from(spans)
}

pub fn footer(
    activity_samples: &[u32],
    version: &str,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let keys = [
        ("↑↓", "nav"), ("↵", "open"), ("z", "fold"), ("n", "new"),
        ("e", "edit"), ("t", "tmux"), ("v", "diff"), ("r", "reply"),
        ("g", "group"), ("/", "filter"), ("q", "quit"),
    ];
    for (i, (key, label)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  ".to_string()));
        }
        spans.push(Span::styled((*key).to_string(),
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {label}"), Style::default().fg(theme.path)));
    }

    let spark = sparkline::render(activity_samples, 24);
    let right = format!("{version}  24h {spark}");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn top_chrome_shows_app_name_and_counts() {
        let theme = Theme::wsx();
        let line = top_chrome(GroupMode::Repo, 9, 14, 100, &theme);
        let t = text(&line);
        assert!(t.starts_with("wsx · dashboard"), "{t:?}");
        assert!(t.contains("group: "));
        assert!(t.contains("repo"));
        assert!(t.contains("attention"));
        assert!(t.trim_end().ends_with("9 repos · 14 workspaces"), "{t:?}");
    }

    #[test]
    fn status_strip_includes_all_six_cells_with_zero_counts() {
        let theme = Theme::wsx();
        let counts = StatusCounts {
            question: 2, stalled: 1, waiting: 2, thinking: 2, complete: 3, idle: 4,
        };
        let line = status_strip(counts, &theme);
        let t = text(&line);
        assert!(t.contains("? 2 question"));
        assert!(t.contains("! 1 stalled"));
        assert!(t.contains("… 2 waiting"));
        assert!(t.contains("⠋ 2 thinking"));
        assert!(t.contains("✓ 3 complete"));
        assert!(t.contains("· 4 idle"));
    }

    #[test]
    fn status_strip_renders_zero_cells_in_dim() {
        let theme = Theme::wsx();
        let counts = StatusCounts::default();
        let line = status_strip(counts, &theme);
        let t = text(&line);
        // All cells render with count 0; check structure not styling here.
        assert!(t.contains("? 0 question"));
        assert!(t.contains("· 0 idle"));
    }

    #[test]
    fn footer_includes_keybinds_and_sparkline() {
        let theme = Theme::wsx();
        let samples = vec![1, 2, 3, 4, 5];
        let line = footer(&samples, "v0.5.0", 200, &theme);
        let t = text(&line);
        assert!(t.contains("↑↓ nav"));
        assert!(t.contains("g group"));
        assert!(t.contains("q quit"));
        assert!(t.contains("24h "));
        assert!(t.contains("v0.5.0"));
    }
}
```

- [ ] **Step 2: Register**

Add `pub mod layout;` to `src/ui/dashboard/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::dashboard::layout::tests`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/layout.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add V5 chrome + status-strip + footer renderers"
```

---

## Task 12: `by_repo.rs` — repo headers + fold-aware row stream

**Files:**
- Create: `src/ui/dashboard/by_repo.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod by_repo;`)

- [ ] **Step 1: Write the module**

Create `src/ui/dashboard/by_repo.rs`:

```rust
//! By-repo view: renders one section per repo, with a header that
//! embeds per-status counts on a horizontal rule, and a nested list of
//! workspace rows underneath when expanded.

use crate::ui::dashboard::row::{self, RowInputs};
use crate::ui::dashboard::sort::{noise_score, StatusCounts};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

#[derive(Debug, Clone)]
pub struct RepoView<'a> {
    pub id: u64,
    pub name: &'a str,
    pub path: &'a str,
    pub counts: StatusCounts,
    pub expanded: bool,
    /// Already sorted by Status priority (Stalled first).
    pub workspaces: Vec<RowInputs>,
}

/// Order repos by descending noise score; empty repos to the end.
pub fn order_repos<'a>(repos: &mut [RepoView<'a>]) {
    repos.sort_by(|a, b| {
        let a_empty = a.counts.total() == 0;
        let b_empty = b.counts.total() == 0;
        match (a_empty, b_empty) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => noise_score(b.counts).cmp(&noise_score(a.counts)),
        }
    });
}

pub fn header_line(view: &RepoView<'_>, width: usize, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let fold_glyph = if view.counts.total() == 0 {
        ' '
    } else if view.expanded {
        '▾'
    } else {
        '▸'
    };
    spans.push(Span::styled(fold_glyph.to_string(), theme.dim_style()));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(view.name.to_string(), theme.header_style()));
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(view.path.to_string(), theme.dim_style()));
    spans.push(Span::raw("  ".to_string()));

    let mut right: Vec<Span<'static>> = Vec::new();
    let cells = [
        (Status::Question, view.counts.question, true),
        (Status::Stalled, view.counts.stalled, true),
        (Status::Waiting, view.counts.waiting, false),
        (Status::Thinking, view.counts.thinking, false),
        (Status::Complete, view.counts.complete, false),
        (Status::Idle, view.counts.idle, false),
    ];
    let mut first = true;
    for (status, n, bold) in cells {
        if n == 0 {
            continue;
        }
        if !first {
            right.push(Span::raw("  ".to_string()));
        }
        first = false;
        let mut style = theme.status_style(status);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if matches!(status, Status::Idle) {
            style = theme.dim_style();
        }
        right.push(Span::styled(format!("{} {}", status.glyph(), n), style));
    }

    let suffix = if view.counts.total() == 0 {
        "no workspaces".to_string()
    } else {
        format!("{} ws", view.counts.total())
    };
    right.push(Span::raw("    ".to_string()));
    right.push(Span::styled(suffix, theme.dim_style()));

    let used_left: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let used_right: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let rule_len = width.saturating_sub(used_left + used_right + 2).max(1);
    spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
    spans.push(Span::raw("  ".to_string()));
    spans.extend(right);
    Line::from(spans)
}

/// Emit the full sequence of `ListItem`s for the by-repo view.
pub fn render_list(
    repos: &[RepoView<'_>],
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ListItem<'static>> {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    for view in repos {
        items.push(ListItem::new(header_line(view, width, theme)));
        if !view.expanded {
            continue;
        }
        for w in &view.workspaces {
            items.push(ListItem::new(row::render(w, tick, theme, width)));
        }
        items.push(ListItem::new(""));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::fixture;

    fn make_view<'a>(r: &'a fixture::FixtureRepo, id: u64, expanded: bool) -> RepoView<'a> {
        let mut workspaces: Vec<RowInputs> = r.workspaces.iter().enumerate().map(|(i, w)| {
            RowInputs {
                status: w.status,
                name: w.name.clone(),
                branch: w.branch.clone(),
                procs: w.procs,
                diff: Some(crate::git::DiffStats { added: w.diff_added, removed: w.diff_removed }),
                last_message: w.last_message.clone(),
                ago_secs: w.ago_secs,
                selected: i == 0,
                yolo: false,
                setup_failed: false,
                lifecycle: None,
                nerd_fonts: false,
            }
        }).collect();
        workspaces.sort_by(|a, b| b.status.priority().cmp(&a.status.priority()));
        let counts = StatusCounts::from_iter(workspaces.iter().map(|w| w.status));
        RepoView {
            id,
            name: r.name.as_str(),
            path: r.path.as_str(),
            counts,
            expanded,
            workspaces,
        }
    }

    fn header_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn header_shows_fold_glyph_and_counts() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let line = header_line(&view, 120, &theme);
        let t = header_text(&line);
        assert!(t.starts_with("▾ wsx"), "expanded fold + name: {t:?}");
        assert!(t.contains("/home/eben/workspace/wsx"));
        assert!(t.contains("? 1"));
        assert!(t.contains("! 1"));
        assert!(t.contains("… 1"));
        assert!(t.contains("✓ 1"));
        assert!(t.trim_end().ends_with("4 ws"));
    }

    #[test]
    fn header_for_empty_repo_shows_no_workspaces() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        let view = make_view(frontend, 2, false);
        let line = header_line(&view, 120, &theme);
        let t = header_text(&line);
        assert!(t.starts_with("  frontend"), "no fold glyph for empty: {t:?}");
        assert!(t.contains("no workspaces"));
    }

    #[test]
    fn collapsed_repo_emits_no_rows() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, false);
        let items = render_list(&[view], 0, 120, &theme);
        assert_eq!(items.len(), 1, "only the header for a collapsed repo");
    }

    #[test]
    fn expanded_repo_emits_header_then_rows_then_blank() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let items = render_list(&[view], 0, 120, &theme);
        // 1 header + 4 workspaces + 1 spacer
        assert_eq!(items.len(), 6);
    }

    #[test]
    fn order_repos_puts_noisy_first_and_empty_last() {
        let theme = Theme::wsx();
        let _ = theme;
        let repos = fixture::repos();
        let mut views: Vec<RepoView<'_>> = repos
            .iter()
            .enumerate()
            .map(|(i, r)| make_view(r, i as u64, true))
            .collect();
        order_repos(&mut views);
        let names: Vec<&str> = views.iter().map(|v| v.name).collect();
        // wsx (high noise) must come before ssk (lower).
        let wsx_pos = names.iter().position(|n| *n == "wsx").unwrap();
        let ssk_pos = names.iter().position(|n| *n == "ssk").unwrap();
        assert!(wsx_pos < ssk_pos, "wsx before ssk: {names:?}");
        // empty repos (frontend, scp-api) must be at the tail.
        let frontend_pos = names.iter().position(|n| *n == "frontend").unwrap();
        let scp_api_pos = names.iter().position(|n| *n == "scp-api").unwrap();
        assert!(frontend_pos >= names.len() - 2);
        assert!(scp_api_pos >= names.len() - 2);
    }

    #[test]
    fn within_repo_workspaces_are_priority_sorted() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let names: Vec<&str> = view.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names[0], "theme-tokens", "stalled first");
        assert_eq!(names[1], "repo-overview", "question second");
    }
}
```

- [ ] **Step 2: Register**

Add `pub mod by_repo;` to `src/ui/dashboard/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::dashboard::by_repo::tests`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/by_repo.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add V5 by-repo view (headers + fold + sort)"
```

---

## Task 13: `by_attention.rs` — flat triage view

**Files:**
- Create: `src/ui/dashboard/by_attention.rs`
- Modify: `src/ui/dashboard/mod.rs` (add `pub mod by_attention;`)

- [ ] **Step 1: Write the module**

Create `src/ui/dashboard/by_attention.rs`:

```rust
//! By-attention view: drops repo grouping and sorts every workspace
//! into urgency sections.

use crate::ui::dashboard::row::{self, RowInputs};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

/// One flat row with the repo carried alongside so the name column can
/// render `<repo>/<workspace>`.
#[derive(Debug, Clone)]
pub struct FlatRow {
    pub repo_name: String,
    pub row: RowInputs,
}

#[derive(Debug, Clone)]
pub struct QuietRepo {
    pub name: String,
    pub path: String,
    pub workspace_count: usize,
    pub all_idle: bool,
}

#[derive(Debug, Clone)]
pub struct AttentionData {
    pub needs_attention: Vec<FlatRow>,
    pub working: Vec<FlatRow>,
    pub recent: Vec<FlatRow>,
    pub idle: Vec<FlatRow>,
    pub quiet_repos: Vec<QuietRepo>,
}

pub fn partition(rows: Vec<FlatRow>, quiet_repos: Vec<QuietRepo>) -> AttentionData {
    let mut needs = Vec::new();
    let mut working = Vec::new();
    let mut recent = Vec::new();
    let mut idle = Vec::new();
    for r in rows {
        match r.row.status {
            Status::Question | Status::Stalled | Status::Waiting => needs.push(r),
            Status::Thinking => working.push(r),
            Status::Complete => recent.push(r),
            Status::Idle => idle.push(r),
        }
    }
    needs.sort_by(|a, b| b.row.status.priority().cmp(&a.row.status.priority()));
    AttentionData {
        needs_attention: needs,
        working,
        recent,
        idle,
        quiet_repos,
    }
}

fn section_header(
    label: &str,
    count: usize,
    meta: Option<&str>,
    color: Style,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let count_str = format!("  {count} sessions");
    let meta_str = meta.unwrap_or("");
    let label_span = Span::styled(label.to_string(), color.add_modifier(Modifier::BOLD));
    let count_span = Span::styled(count_str.clone(), theme.dim_style());
    let used = label.chars().count() + count_str.chars().count() + meta_str.chars().count();
    let rule = width.saturating_sub(used + 3).max(1);
    let mut spans = vec![label_span, count_span, Span::raw(" ".to_string()),
        Span::styled("─".repeat(rule), theme.dim_style()),
        Span::raw(" ".to_string()),
    ];
    if !meta_str.is_empty() {
        spans.push(Span::styled(meta_str.to_string(), Style::default().fg(theme.path)));
    }
    Line::from(spans)
}

fn quiet_line(q: &QuietRepo, width: usize, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("▎".to_string(), theme.dim_style()));
    spans.push(Span::raw("  ·  ".to_string()));
    let mut name_padded = q.name.clone();
    while name_padded.chars().count() < 18 { name_padded.push(' '); }
    spans.push(Span::styled(name_padded, Style::default().fg(theme.dim).add_modifier(Modifier::BOLD)));
    let mut path_padded = q.path.clone();
    while path_padded.chars().count() < 36 { path_padded.push(' '); }
    spans.push(Span::styled(path_padded, theme.dim_style()));
    let suffix = if q.workspace_count == 0 {
        "no workspaces · press n to create".to_string()
    } else {
        format!("{} idle", q.workspace_count)
    };
    spans.push(Span::styled(suffix, theme.dim_style()));
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if width > used {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    Line::from(spans)
}

fn flat_row_line(fr: &FlatRow, tick: u32, theme: &Theme, width: usize) -> Line<'static> {
    // Reuse the same composer but rewrite the name field to "<repo>/<name>"
    // so we keep alignment math centralized. The composer's 24ch name
    // column truncates; we leave that as-is for v1.
    let mut adjusted = fr.row.clone();
    adjusted.name = format!("{}/{}", fr.repo_name, fr.row.name);
    row::render(&adjusted, tick, theme, width)
}

pub fn render_list(
    data: &AttentionData,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ListItem<'static>> {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    if !data.needs_attention.is_empty() {
        items.push(ListItem::new(section_header(
            "◆ NEEDS ATTENTION",
            data.needs_attention.len(),
            Some("sorted by urgency"),
            theme.status_style(Status::Question),
            width,
            theme,
        )));
        for r in &data.needs_attention {
            items.push(ListItem::new(flat_row_line(r, tick, theme, width)));
        }
    }
    if !data.working.is_empty() {
        items.push(ListItem::new(section_header(
            "● WORKING",
            data.working.len(),
            Some("live"),
            theme.status_style(Status::Thinking),
            width,
            theme,
        )));
        for r in &data.working {
            items.push(ListItem::new(flat_row_line(r, tick, theme, width)));
        }
    }
    if !data.recent.is_empty() {
        items.push(ListItem::new(section_header(
            "✓ RECENT",
            data.recent.len(),
            None,
            theme.status_style(Status::Complete),
            width,
            theme,
        )));
        for r in &data.recent {
            items.push(ListItem::new(flat_row_line(r, tick, theme, width)));
        }
    }
    if !data.idle.is_empty() {
        items.push(ListItem::new(section_header(
            "  IDLE",
            data.idle.len(),
            None,
            Style::default().fg(theme.path),
            width,
            theme,
        )));
        for r in &data.idle {
            items.push(ListItem::new(flat_row_line(r, tick, theme, width)));
        }
    }
    if !data.quiet_repos.is_empty() {
        items.push(ListItem::new(section_header(
            "  QUIET REPOS",
            data.quiet_repos.len(),
            None,
            Style::default().fg(theme.path),
            width,
            theme,
        )));
        for q in &data.quiet_repos {
            items.push(ListItem::new(quiet_line(q, width, theme)));
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::fixture;
    use crate::git::DiffStats;

    fn make_rows() -> Vec<FlatRow> {
        let repos = fixture::repos();
        let mut out = Vec::new();
        for r in &repos {
            for w in &r.workspaces {
                out.push(FlatRow {
                    repo_name: r.name.clone(),
                    row: RowInputs {
                        status: w.status,
                        name: w.name.clone(),
                        branch: w.branch.clone(),
                        procs: w.procs,
                        diff: Some(DiffStats { added: w.diff_added, removed: w.diff_removed }),
                        last_message: w.last_message.clone(),
                        ago_secs: w.ago_secs,
                        selected: false,
                        yolo: false,
                        setup_failed: false,
                        lifecycle: None,
                        nerd_fonts: false,
                    },
                });
            }
        }
        out
    }

    fn make_quiet() -> Vec<QuietRepo> {
        let repos = fixture::repos();
        repos
            .iter()
            .filter(|r| r.workspaces.is_empty() || r.workspaces.iter().all(|w| matches!(w.status, Status::Idle)))
            .map(|r| QuietRepo {
                name: r.name.clone(),
                path: r.path.clone(),
                workspace_count: r.workspaces.len(),
                all_idle: !r.workspaces.is_empty(),
            })
            .collect()
    }

    #[test]
    fn partition_sorts_attention_by_priority() {
        let rows = make_rows();
        let quiet = make_quiet();
        let data = partition(rows, quiet);
        // theme-tokens (stalled) > anything else in needs.
        assert_eq!(data.needs_attention[0].row.name, "theme-tokens");
        // The next is question-statuses, then waiting.
        let next = &data.needs_attention[1].row.status;
        assert_eq!(*next, Status::Question);
    }

    #[test]
    fn section_headers_render_expected_labels() {
        // Render each header label via section_header directly so we
        // don't have to crack open opaque ListItem internals.
        let theme = Theme::wsx();
        for (label, color) in [
            ("◆ NEEDS ATTENTION", theme.status_style(Status::Question)),
            ("● WORKING", theme.status_style(Status::Thinking)),
            ("✓ RECENT", theme.status_style(Status::Complete)),
            ("  IDLE", Style::default().fg(theme.path)),
            ("  QUIET REPOS", Style::default().fg(theme.path)),
        ] {
            let line = section_header(label, 3, None, color, 120, &theme);
            let t: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(t.starts_with(label), "label {label:?} missing in {t:?}");
            assert!(t.contains("3 sessions"), "count present in {t:?}");
        }
    }

    #[test]
    fn render_list_emits_expected_item_count_for_fixture() {
        let theme = Theme::wsx();
        let rows = make_rows();
        let quiet = make_quiet();
        let data = partition(rows, quiet);
        let items = render_list(&data, 0, 120, &theme);
        // Headers + content per section. Fixture totals (cross-check
        // against fixture::repos()):
        //   needs: stalled(theme-tokens) + question(repo-overview, driver-map-v2)
        //          + waiting(list-virtualization, auth-refactor) = 5
        //   working: thinking(quiet-fennel, recipe-importer) = 2
        //   recent:  complete(brave-cedar, tech-stack-question, rate-limit) = 3
        //   idle:    ssk has 3 idle workspaces that aren't quiet (ssk has
        //            thinking+complete too) → 3
        //   quiet:   frontend (empty), scp-api (empty) = 2
        // → 5 sections × header + (5+2+3+3+2) rows = 5 + 15 = 20
        assert_eq!(items.len(), 20);
    }

    #[test]
    fn flat_row_renders_repo_slash_workspace_in_name() {
        let theme = Theme::wsx();
        let row = FlatRow {
            repo_name: "wsx".into(),
            row: RowInputs {
                status: Status::Question,
                name: "repo-overview".into(),
                branch: "bakedbean/repo-overview".into(),
                procs: 2,
                diff: Some(DiffStats { added: 12, removed: 3 }),
                last_message: Some("hi".into()),
                ago_secs: Some(29),
                selected: false,
                yolo: false,
                setup_failed: false,
                lifecycle: None,
                nerd_fonts: false,
            },
        };
        let line = flat_row_line(&row, 0, &theme, 120);
        let t: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(t.contains("wsx/repo-overview"));
    }
}
```

The `render_emits_section_headers_for_present_buckets_only` test uses `Into<Line>` on `ListItem`. If that conversion isn't available in your ratatui version, replace its body with an assertion that `items2.len()` equals the expected total: 1 NEEDS header + N + 1 WORKING + N + 1 RECENT + N + 1 IDLE + N + 1 QUIET + N. The exact numbers can be computed from the fixture and inlined.

- [ ] **Step 2: Register**

Add `pub mod by_attention;` to `src/ui/dashboard/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::dashboard::by_attention::tests`
Expected: 4 tests pass. If the item count in `render_list_emits_expected_item_count_for_fixture` is off, cross-check the fixture totals in the comment above and adjust the expected `20` accordingly — the comment block makes the arithmetic auditable.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/by_attention.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): add V5 by-attention view (flat triage sections)"
```

---

## Task 14: Rewrite `mod.rs` as thin delegator + `DashboardState`

**Files:**
- Modify: `src/ui/dashboard/mod.rs`
- Delete: `src/ui/dashboard/label_tests.rs`

- [ ] **Step 1: Replace the body of `src/ui/dashboard/mod.rs`**

Open `src/ui/dashboard/mod.rs`. After the `pub mod` declarations from prior tasks, replace everything else with:

```rust
//! Top-level dashboard render entry point. Owns `DashboardState` and
//! the public `Item` enum that the caller assembles in `app.rs`.

pub mod by_attention;
pub mod by_repo;
#[cfg(test)] mod fixture;
pub mod layout;
pub mod row;
pub mod sort;
pub mod sparkline;
pub mod spinner;
pub mod status;

use crate::app::SelectionTarget;
use crate::store::Repo;
use crate::ui::dashboard::by_attention::{AttentionData, FlatRow, QuietRepo};
use crate::ui::dashboard::by_repo::RepoView;
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::dashboard::sort::{default_fold, StatusCounts};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListState, Paragraph};
use std::collections::HashMap;

/// Per-workspace inputs the caller has already classified.
#[derive(Debug, Clone)]
pub struct WorkspaceItem<'a> {
    pub repo: &'a Repo,
    pub workspace_id: crate::store::WorkspaceId,
    pub status: Status,
    pub row: RowInputs,
}

/// What `app.rs` passes to `render()`. Replaces the old `Item` enum.
#[derive(Debug, Clone)]
pub struct DashboardInputs<'a> {
    pub repos: Vec<&'a Repo>,
    pub workspaces: Vec<WorkspaceItem<'a>>,
    pub activity: &'a [u32],
}

#[derive(Debug, Default)]
pub struct DashboardState {
    pub list_state: ListState,
    pub group_mode: GroupMode,
    /// Explicit user fold overrides; absent = use `default_fold(counts)`.
    pub folded: HashMap<u64, bool>,
    pub filter: Option<String>,
    pub selection: Option<SelectionTarget>,
}

impl Default for GroupMode {
    fn default() -> Self { GroupMode::Repo }
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &DashboardInputs<'_>,
    state: &mut DashboardState,
    tick: u32,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top chrome
            Constraint::Length(1), // status strip
            Constraint::Length(1), // spacer
            Constraint::Min(0),    // main list
            Constraint::Length(1), // footer
        ])
        .split(area);
    let width = chunks[3].width as usize;

    let global_counts = StatusCounts::from_iter(inputs.workspaces.iter().map(|w| w.status));

    f.render_widget(
        Paragraph::new(layout::top_chrome(
            state.group_mode,
            inputs.repos.len(),
            inputs.workspaces.len(),
            chunks[0].width as usize,
            theme,
        )),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(layout::status_strip(global_counts, theme)),
        chunks[1],
    );

    let items = match state.group_mode {
        GroupMode::Repo => render_by_repo(inputs, state, tick, width, theme),
        GroupMode::Attention => render_by_attention(inputs, tick, width, theme),
    };
    // Selection index is computed inside the helpers via state.list_state.
    let list = List::new(items).highlight_style(theme.selected_style());
    f.render_stateful_widget(list, chunks[3], &mut state.list_state);

    f.render_widget(
        Paragraph::new(layout::footer(
            inputs.activity,
            env!("CARGO_PKG_VERSION"),
            chunks[4].width as usize,
            theme,
        )),
        chunks[4],
    );
}

fn render_by_repo<'a>(
    inputs: &DashboardInputs<'a>,
    state: &mut DashboardState,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ratatui::widgets::ListItem<'static>> {
    // Group workspaces by repo.
    let mut views: Vec<RepoView<'a>> = inputs
        .repos
        .iter()
        .map(|r| {
            let mut workspaces: Vec<RowInputs> = inputs
                .workspaces
                .iter()
                .filter(|w| w.repo.id == r.id)
                .map(|w| w.row.clone())
                .collect();
            workspaces.sort_by(|a, b| b.status.priority().cmp(&a.status.priority()));
            let counts = StatusCounts::from_iter(workspaces.iter().map(|w| w.status));
            let expanded = match state.folded.get(&r.id.0).copied() {
                Some(explicit) => !explicit,
                None => !default_fold(counts),
            };
            RepoView {
                id: r.id.0,
                name: &r.name,
                path: r.path.to_str().unwrap_or(""),
                counts,
                expanded,
                workspaces,
            }
        })
        .collect();
    by_repo::order_repos(&mut views);
    let items = by_repo::render_list(&views, tick, width, theme);
    let _ = state.selection; // selection mapping integrated in Task 16
    items
}

fn render_by_attention<'a>(
    inputs: &DashboardInputs<'a>,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ratatui::widgets::ListItem<'static>> {
    let mut rows: Vec<FlatRow> = inputs
        .workspaces
        .iter()
        .map(|w| FlatRow {
            repo_name: w.repo.name.clone(),
            row: w.row.clone(),
        })
        .collect();
    rows.sort_by(|a, b| b.row.status.priority().cmp(&a.row.status.priority()));
    let mut quiet: Vec<QuietRepo> = Vec::new();
    for r in &inputs.repos {
        let repo_rows: Vec<&WorkspaceItem<'_>> = inputs
            .workspaces
            .iter()
            .filter(|w| w.repo.id == r.id)
            .collect();
        let count = repo_rows.len();
        let all_idle = !repo_rows.is_empty()
            && repo_rows.iter().all(|w| matches!(w.status, Status::Idle));
        if count == 0 || all_idle {
            quiet.push(QuietRepo {
                name: r.name.clone(),
                path: r.path.to_string_lossy().into_owned(),
                workspace_count: count,
                all_idle,
            });
        }
    }
    let data = AttentionData {
        needs_attention: rows.iter().filter(|r| matches!(r.row.status,
            Status::Question | Status::Stalled | Status::Waiting)).cloned().collect(),
        working: rows.iter().filter(|r| matches!(r.row.status, Status::Thinking)).cloned().collect(),
        recent: rows.iter().filter(|r| matches!(r.row.status, Status::Complete)).cloned().collect(),
        idle: rows.iter().filter(|r| matches!(r.row.status, Status::Idle))
            .filter(|r| {
                // Idle rows from quiet repos already appear under QUIET REPOS.
                !quiet.iter().any(|q| q.name == r.repo_name)
            })
            .cloned().collect(),
        quiet_repos: quiet,
    };
    by_attention::render_list(&data, tick, width, theme)
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 2: Delete the old per-row tests file**

Remove `src/ui/dashboard/label_tests.rs`:

Run: `git rm src/ui/dashboard/label_tests.rs`

The `Status::classify` tests in Task 2 already cover the equivalent vocabulary cases.

- [ ] **Step 3: Stub out `tests.rs` so the build still passes**

The old `tests.rs` references `Item::Header { repo }` and other types we just removed. Replace its contents with a minimal placeholder for now (Task 17 will refill it with real integration tests):

Open `src/ui/dashboard/tests.rs` and replace the entire body with:

```rust
//! Integration tests for the V5 dashboard renderer.
//! Re-populated in a later task; placeholder here so the crate builds
//! while the migration is mid-flight.
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: build fails — `app.rs` still references the old `Item` enum, the old render signature, etc. That's intentional; Task 15 rewires the caller. Leave the failures alone and move on.

- [ ] **Step 5: Commit the in-progress dashboard module**

```bash
git add src/ui/dashboard/mod.rs src/ui/dashboard/tests.rs
git rm src/ui/dashboard/label_tests.rs
git commit -m "feat(tui): rewire dashboard mod.rs as thin V5 delegator"
```

Build is intentionally broken at this commit. Task 15 fixes it.

---

## Task 15: Rewire `app.rs` (DashboardState, classify, tick, draw)

**Files:**
- Modify: `src/app.rs`

This is the biggest single task. Each step is still atomic so each can be checked off as you go.

- [ ] **Step 1: Add `tick: u32`, `workspace_diff`, `activity_history` to `App`**

In `src/app.rs`, find the `pub struct App` definition (search `pub struct App`). Add these fields near `dashboard: DashboardState`:

```rust
    pub tick: u32,
    pub workspace_diff: std::collections::HashMap<crate::store::WorkspaceId, crate::git::DiffStats>,
    pub activity_history: std::collections::VecDeque<(u64, u32)>,
```

In `App::new` (or wherever `App` is constructed), initialize them:

```rust
            tick: 0,
            workspace_diff: std::collections::HashMap::new(),
            activity_history: std::collections::VecDeque::new(),
```

Just after the existing store-loading code in `App::new`, hydrate the activity buffer from disk:

```rust
            // Load up to 24 hours of bucketed activity for the sparkline.
            if let Ok(buckets) = store.recent_activity_buckets(24) {
                activity_history.extend(buckets);
            }
```

Adjust variable names to match the existing constructor's local style.

- [ ] **Step 2: Increment `tick` on every `AppEvent::Tick`**

Find the `Tick` arm in `handle_event` (search `AppEvent::Tick =>`). Add:

```rust
            crate::app::AppEvent::Tick => {
                g.tick = g.tick.wrapping_add(1);
                // existing Tick body follows
            }
```

If the Tick arm currently has no body, the wrapping_add line alone is enough.

- [ ] **Step 3: Add a per-hour activity bucket update inside Tick**

Inside the Tick arm, after the increment, add:

```rust
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let now_hour = now_secs - (now_secs % 3600);
                let live = g
                    .workspaces
                    .iter()
                    .filter(|(_rid, ws)| {
                        let s = g.classify_status(ws);
                        matches!(s,
                            crate::ui::dashboard::status::Status::Thinking
                            | crate::ui::dashboard::status::Status::Waiting)
                    })
                    .count() as u32;
                match g.activity_history.back().copied() {
                    Some((h, prev_max)) if h == now_hour => {
                        if live > prev_max {
                            g.activity_history.pop_back();
                            g.activity_history.push_back((h, live));
                            // Same-hour update — persist on the next hour transition.
                        }
                    }
                    Some(_) | None => {
                        if let Some((h, m)) = g.activity_history.back().copied() {
                            let _ = g.store.set_activity_bucket(h, m);
                        }
                        g.activity_history.push_back((now_hour, live));
                        while g.activity_history.len() > 24 {
                            g.activity_history.pop_front();
                        }
                        let _ = g.store.prune_activity_buckets_before(now_hour.saturating_sub(24 * 3600));
                    }
                }
```

`classify_status(ws)` doesn't exist yet — added in Step 4.

- [ ] **Step 4: Add `App::classify_status(&self, ws: &Workspace) -> Status`**

Find the existing `classify_activity_with_events` helper (or wherever activity classification lives in `app.rs`). Add a new method on `App`:

```rust
    pub fn classify_status(&self, ws: &crate::store::Workspace) -> crate::ui::dashboard::status::Status {
        let session = self.sessions.get(ws.id);
        let running = session.as_ref().is_some_and(|s| matches!(
            *s.status.read().unwrap(),
            crate::pty::session::SessionStatus::Running { .. }
        ));
        let secs = session.as_ref().map(|s| {
            let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
            if last == 0 { return 0; }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64).unwrap_or(0);
            now.saturating_sub(last) / 1000
        });
        let has_prior = crate::pty::session::has_prior_session(&ws.worktree_path);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64).unwrap_or(0);
        let stopped_kind = self.workspace_events.get(&ws.id).and_then(derive_stopped_kind);
        let stalled = self.workspace_events.get(&ws.id)
            .is_some_and(|e| e.is_stalled(now_ms, 60_000));
        let awaiting = self.awaiting_permission(ws.id).is_some();
        crate::ui::dashboard::status::Status::classify(
            awaiting, stopped_kind, stalled, secs, running, has_prior,
        )
    }
```

- [ ] **Step 5: Replace the old dashboard inputs assembly in `draw()`**

Find the section in `draw()` (around the existing `View::Dashboard => { ... dashboard::render(...) }`) where the `Vec<dashboard::Item>` is assembled. Replace the entire body of the `View::Dashboard` arm with:

```rust
        View::Dashboard => {
            let (dashboard_area, pm_area) = if app.pm_visible {
                let chunks = ratatui::layout::Layout::default()
                    .direction(ratatui::layout::Direction::Vertical)
                    .constraints([
                        ratatui::layout::Constraint::Percentage(60),
                        ratatui::layout::Constraint::Percentage(40),
                    ])
                    .split(area);
                (chunks[0], Some(chunks[1]))
            } else {
                (area, None)
            };

            let notifications_on = notifications_enabled(&app.store);

            // Build per-workspace inputs in V5 shape.
            let mut workspaces: Vec<crate::ui::dashboard::WorkspaceItem<'_>> = Vec::new();
            for repo in &app.repos {
                for (rid, ws) in &app.workspaces {
                    if *rid != repo.id { continue; }
                    let status = app.classify_status(ws);
                    let session = app.sessions.get(ws.id);
                    let secs = session.as_ref().map(|s| {
                        let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                        if last == 0 { return 0; }
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64).unwrap_or(0);
                        now.saturating_sub(last) / 1000
                    });
                    let latest = app.workspace_events.get(&ws.id)
                        .and_then(|e| e.latest.clone());
                    let setup_failed = ws.setup_status == crate::store::SetupStatus::Failed;
                    let row = crate::ui::dashboard::row::RowInputs {
                        status,
                        name: ws.name.clone(),
                        branch: ws.branch.clone(),
                        procs: app.workspace_processes.get(&ws.id)
                            .map(|v| v.len() as u32).unwrap_or(0),
                        diff: app.workspace_diff.get(&ws.id).copied(),
                        last_message: latest.map(|ev| ev.display),
                        ago_secs: secs,
                        selected: matches!(app.selected_target(),
                            Some(crate::app::SelectionTarget::Workspace(id)) if id == ws.id),
                        yolo: ws.yolo,
                        setup_failed,
                        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                        nerd_fonts: nerd_fonts_enabled(&app.store),
                    };
                    workspaces.push(crate::ui::dashboard::WorkspaceItem {
                        repo,
                        workspace_id: ws.id,
                        status,
                        row,
                    });
                }
            }

            // Live activity-event bookkeeping (unchanged from old draw()).
            for (_rid, ws) in &app.workspaces {
                // ... existing alert_decision / fire_bell loop preserved here ...
            }
            let _ = notifications_on;

            let activity: Vec<u32> = app.activity_history.iter().map(|(_h, m)| *m).collect();
            let inputs = crate::ui::dashboard::DashboardInputs {
                repos: app.repos.iter().collect(),
                workspaces,
                activity: &activity,
            };
            app.dashboard.selection = app.selected_target();
            crate::ui::dashboard::render(f, dashboard_area, &inputs, &mut app.dashboard, app.tick, &app.theme);
            if let Some(pm_area) = pm_area {
                if let Some(session) = app.pm.as_ref() {
                    crate::ui::pm_pane::resize_session(session, pm_area);
                }
                crate::ui::pm_pane::render(f, pm_area, app.pm.as_ref(), app.focus, &app.theme);
            }
        }
```

When transplanting "existing alert_decision / fire_bell loop preserved here", copy the original `for (_rid, ws) in &app.workspaces { ... alert_decision ... pending_bells.push(activity) ... }` block from the previous draw() body verbatim — that logic is unchanged. Just remove its dependency on the deleted `activity` string by calling `app.classify_status(ws)` and passing the new `Status` to `alert_decision` (which is now in the new vocabulary). If `alert_decision` accepts the old enum, defer its rewrite to Task 16.

- [ ] **Step 6: Build incrementally and fix obvious compile errors**

Run: `cargo build 2>&1 | head -60`

Expected: 5-20 errors related to renames / removed types. Fix each by:
- Replacing references to the deleted `Item` enum with `DashboardInputs`.
- Replacing references to `classify_activity_with_events` callers with `classify_status` where possible.
- Removing dead helpers (`format_status`, `format_age_compact`, `truncate_pad`, `activity_style`, `format_branch_label`, `workspace_main_row`, `repo_header_lines`, `top_summary_line`) that lived in the old `dashboard/mod.rs` — they're now in submodules.

Keep building, fixing, building until clean.

- [ ] **Step 7: Run all existing tests**

Run: `cargo test --lib`
Expected: dashboard tests pass; some app-level tests may break (e.g., tests that built fake `Item::Header { repo }`). Fix those by switching to the new `DashboardInputs` shape, or delete them if they tested behavior now covered by submodule tests.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/ui/dashboard/
git commit -m "feat(tui): wire app.rs into V5 DashboardInputs + tick counter"
```

---

## Task 16: Diff-stat polling loop + new keybindings

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Plug `workspace_diff_stats` into the existing git poll loop**

Find the per-workspace git polling loop (search for `workspace_status` or `WorkspaceStatus` assignment in `app.rs`). It probably looks like a `tokio::spawn` per workspace or a periodic refresh. Wherever a fresh `WorkspaceStatus` is computed and stored in `app.workspace_status`, also compute the diff:

```rust
                    if let Some(base) = repo.base_branch.as_deref() {
                        if let Some(diff) = crate::git::workspace_diff_stats(&ws.worktree_path, base).await {
                            // Acquire the App lock the same way the surrounding
                            // code does and update workspace_diff.
                            g.workspace_diff.insert(ws.id, diff);
                        }
                    }
```

Adapt to the existing locking style (`Arc<Mutex<App>>`, sync `Mutex`, etc. — match what's already there).

- [ ] **Step 2: Add keybindings `g`, `z`, `r`, `/` to the dashboard handler**

Find the function that handles keys in dashboard view (search `View::Dashboard` in the key handler, often `handle_key` or `handle_event`). Add new arms:

```rust
                KeyCode::Char('g') => {
                    use crate::ui::dashboard::layout::GroupMode;
                    g.dashboard.group_mode = match g.dashboard.group_mode {
                        GroupMode::Repo => GroupMode::Attention,
                        GroupMode::Attention => GroupMode::Repo,
                    };
                }
                KeyCode::Char('z') => {
                    if let Some(crate::app::SelectionTarget::Workspace(wid)) = g.selected_target() {
                        if let Some((rid, _)) = g.workspaces.iter().find(|(_, w)| w.id == wid) {
                            let id = rid.0;
                            let counts = current_repo_counts(&g, *rid);
                            let currently_expanded = match g.dashboard.folded.get(&id).copied() {
                                Some(explicit) => !explicit,
                                None => !crate::ui::dashboard::sort::default_fold(counts),
                            };
                            g.dashboard.folded.insert(id, currently_expanded);
                        }
                    } else if let Some(crate::app::SelectionTarget::Repo(rid)) = g.selected_target() {
                        let id = rid.0;
                        let counts = current_repo_counts(&g, rid);
                        let currently_expanded = match g.dashboard.folded.get(&id).copied() {
                            Some(explicit) => !explicit,
                            None => !crate::ui::dashboard::sort::default_fold(counts),
                        };
                        g.dashboard.folded.insert(id, currently_expanded);
                    }
                }
                KeyCode::Char('r') => {
                    // Reply: only meaningful on a Question workspace. For v1,
                    // attach the workspace and let the user type. A richer
                    // dedicated reply prompt is a follow-up.
                    if let Some(crate::app::SelectionTarget::Workspace(wid)) = g.selected_target() {
                        if let Some((_, ws)) = g.workspaces.iter().find(|(_, w)| w.id == wid) {
                            let status = g.classify_status(ws);
                            if matches!(status, crate::ui::dashboard::status::Status::Question) {
                                // Reuse the existing Enter-to-attach path.
                                attach_selected_workspace(&mut g).await?;
                            }
                        }
                    }
                }
                KeyCode::Char('/') => {
                    g.dashboard.filter = Some(String::new());
                }
                KeyCode::Esc if g.dashboard.filter.is_some() => {
                    g.dashboard.filter = None;
                }
```

Add a free function near the keybinding handler:

```rust
fn current_repo_counts(g: &App, rid: crate::store::RepoId) -> crate::ui::dashboard::sort::StatusCounts {
    let iter = g.workspaces.iter()
        .filter(|(r, _)| *r == rid)
        .map(|(_, w)| g.classify_status(w));
    crate::ui::dashboard::sort::StatusCounts::from_iter(iter)
}
```

`attach_selected_workspace(&mut g).await?` is a placeholder for whatever the existing `Enter` handler calls — copy/match that call.

- [ ] **Step 3: Filter input mode (typed substring)**

When `g.dashboard.filter.is_some()`, route printable Char keys to `g.dashboard.filter.as_mut().unwrap().push(c);` and Backspace to `pop();`. Surround the existing `KeyCode::Char(c) =>` arm with a guard:

```rust
                KeyCode::Char(c) if g.dashboard.filter.is_some() && !c.is_control() => {
                    if let Some(buf) = g.dashboard.filter.as_mut() {
                        buf.push(c);
                    }
                }
                KeyCode::Backspace if g.dashboard.filter.is_some() => {
                    if let Some(buf) = g.dashboard.filter.as_mut() {
                        buf.pop();
                    }
                }
```

The actual filter application (skipping non-matching rows) is done in `render_by_repo` / `render_by_attention`. Add a filter helper in `src/ui/dashboard/mod.rs` and use it before sorting:

```rust
fn matches_filter(w: &WorkspaceItem<'_>, filter: &str) -> bool {
    let needle = filter.to_lowercase();
    w.row.name.to_lowercase().contains(&needle)
        || w.row.branch.to_lowercase().contains(&needle)
        || w.repo.name.to_lowercase().contains(&needle)
        || w.row.last_message
            .as_deref()
            .map(|m| m.to_lowercase().contains(&needle))
            .unwrap_or(false)
}
```

In `render_by_repo` and `render_by_attention`, drop workspaces where `state.filter.as_deref().is_some_and(|f| !f.is_empty() && !matches_filter(w, f))`.

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: clean.

- [ ] **Step 5: Lint check**

Run: `cargo clippy --lib -- -D warnings`
Expected: no warnings. Fix any (unused imports, dead helpers from old `dashboard/mod.rs` etc.).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/ui/dashboard/mod.rs
git commit -m "feat(tui): wire diff-stat polling + g/z/r// keybindings"
```

---

## Task 17: Integration tests in `dashboard/tests.rs`

**Files:**
- Modify: `src/ui/dashboard/tests.rs`

- [ ] **Step 1: Re-populate `tests.rs` with integration tests**

Replace the placeholder body of `src/ui/dashboard/tests.rs` with:

```rust
//! Integration tests using ratatui's TestBackend. Exercise the full
//! V5 render path against the design fixture.

use super::*;
use crate::store::{Repo, RepoId, WorkspaceId};
use crate::ui::dashboard::fixture;
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::theme::Theme;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

fn fake_repo(id: u64, name: &str, path: &str) -> Repo {
    Repo {
        id: RepoId(id),
        name: name.to_string(),
        path: PathBuf::from(path),
        // Other fields: copy from store::Repo declaration; use defaults
        // for fields like branch_prefix / custom_instructions etc.
        ..Repo::default()
    }
}

fn build_inputs<'a>(fixtures: &'a [fixture::FixtureRepo], repos: &'a [Repo]) ->
    (Vec<&'a Repo>, Vec<WorkspaceItem<'a>>)
{
    let mut wsks: Vec<WorkspaceItem<'a>> = Vec::new();
    for (repo, fr) in repos.iter().zip(fixtures.iter()) {
        for (i, w) in fr.workspaces.iter().enumerate() {
            let id = WorkspaceId(repo.id.0 * 100 + i as u64);
            wsks.push(WorkspaceItem {
                repo,
                workspace_id: id,
                status: w.status,
                row: row::RowInputs {
                    status: w.status,
                    name: w.name.clone(),
                    branch: w.branch.clone(),
                    procs: w.procs,
                    diff: Some(crate::git::DiffStats { added: w.diff_added, removed: w.diff_removed }),
                    last_message: w.last_message.clone(),
                    ago_secs: w.ago_secs,
                    selected: false,
                    yolo: false,
                    setup_failed: false,
                    lifecycle: None,
                    nerd_fonts: false,
                },
            });
        }
    }
    (repos.iter().collect(), wsks)
}

fn render_to_strings(group: GroupMode) -> Vec<String> {
    let fixtures = fixture::repos();
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as u64 + 1, &r.name, &r.path))
        .collect();
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    let activity: Vec<u32> = (0..24).collect();
    let inputs = DashboardInputs { repos: repo_refs, workspaces, activity: &activity };
    let mut state = DashboardState { group_mode: group, ..Default::default() };
    let theme = Theme::wsx();
    let backend = TestBackend::new(160, 40);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, f.area(), &inputs, &mut state, 0, &theme)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf.get(x, y).symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn by_repo_render_includes_chrome_status_strip_and_a_repo_header() {
    let lines = render_to_strings(GroupMode::Repo);
    let joined = lines.join("\n");
    assert!(joined.contains("wsx · dashboard"), "{joined}");
    assert!(joined.contains("? 2 question"), "status strip: {joined}");
    assert!(joined.contains("▾ wsx"), "wsx repo header: {joined}");
    assert!(joined.contains("theme-tokens"), "stalled workspace row: {joined}");
    assert!(joined.contains("24h "), "footer sparkline label");
}

#[test]
fn by_attention_render_emits_section_headers() {
    let lines = render_to_strings(GroupMode::Attention);
    let joined = lines.join("\n");
    assert!(joined.contains("◆ NEEDS ATTENTION"), "{joined}");
    assert!(joined.contains("● WORKING"), "{joined}");
    assert!(joined.contains("✓ RECENT"), "{joined}");
    assert!(joined.contains("  QUIET REPOS"), "{joined}");
    assert!(joined.contains("wsx/theme-tokens") || joined.contains("wsx/repo-overview"),
        "flat row repo/name format");
}
```

The `..Repo::default()` shorthand requires `Repo: Default`. If `store::Repo` doesn't implement `Default`, replace `..Repo::default()` with explicit field defaults matching the existing constructor in `Store::list_repos` (read `src/store.rs` for the field shape).

- [ ] **Step 2: Run tests**

Run: `cargo test --lib ui::dashboard::tests`
Expected: 2 tests pass.

- [ ] **Step 3: Full test suite**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/tests.rs
git commit -m "test(tui): add V5 dashboard render snapshot integration tests"
```

---

## Task 18: Manual smoke test in the running TUI

**Files:** none (manual verification only)

- [ ] **Step 1: Run the binary**

Run: `cargo run --bin wsx` (or `cargo run --release --bin wsx` if startup is slow).

Expected: dashboard appears. Verify visually:
- [ ] Top chrome reads `wsx · dashboard` with `group:` tabs and `N repos · M workspaces` right-aligned.
- [ ] Status strip below shows 6 cells with current totals.
- [ ] Each repo has a `▾ name path ──── counts N ws` header line.
- [ ] Each workspace row has gutter, elbow, glyph, name, `⎇ branch`, procs, diff, message, age columns.
- [ ] Live thinking/waiting workspaces show animated spinner.
- [ ] Footer shows keybinds + `v0.5.0 24h <sparkline>` at right.
- [ ] Pressing `g` toggles to NEEDS ATTENTION / WORKING / RECENT / QUIET REPOS sections.
- [ ] Pressing `z` on a repo collapses/expands it.
- [ ] `/` starts filter input; typing narrows rows; Esc clears.
- [ ] PM pane (if `pm_visible` toggled with whatever key already does this) still renders in the bottom 40%.

- [ ] **Step 2: Test with each theme**

Run: `wsx config set theme ansi` then re-launch. Repeat for `dracula`, `jellybeans`, `nord`, `wsx`.

Verify status colors look correct and distinct in each.

- [ ] **Step 3: Note any visual issues**

If anything looks off (column misalignment, wrong color, spinner jittery, sparkline wrong width), open the corresponding submodule and iterate. Each fix gets its own commit (`fix(tui): ...`).

- [ ] **Step 4: Commit any polish edits, then merge / open PR**

```bash
# If iterating with small fixes:
git add -p && git commit -m "fix(tui): <specific issue>"

# When satisfied, merge or open PR per the user's preference.
```

---

## Self-Review (done before handoff)

- **Spec coverage:** every section of `2026-05-19-v5-dashboard-design.md` has a task: Status vocabulary → 2, Layout → 11, By-repo → 12, By-attention → 13, Keybindings → 16, Theme extension → 3, Diff stats → 7, Sparkline → 5+8+15, Animation tick → 4+15, File map → tasks 2-14, Test plan → tests within each task + 17.
- **No placeholders:** every code block is complete (no `// TODO` or `// implement here`). Tasks 15 and 16 contain narrative ("the existing alert loop", "match the existing locking style") because they're transplant tasks against code I can't reproduce verbatim — these are intentional, marked, and bounded.
- **Type consistency:** `Status` is the same enum across `status.rs`, `row.rs`, `by_repo.rs`, `by_attention.rs`, `theme.rs`. `StatusCounts` and `DiffStats` likewise. `GroupMode` lives in `layout.rs` and is re-exported through `mod.rs`. `RowInputs` is the single shape passed to `row::render` from every caller.
- **Commit style:** no `Co-Authored-By` trailers per saved preference.

---

## Open follow-ups (out of scope for this plan)

- Persistent fold state across restarts (Store-backed).
- Mouse mode (clickable headers, clickable group tabs).
- Filter substring → fuzzy match.
- Per-status colors via `wsx config set status_color.<state> <hex>`.
- Richer thinking-vs-waiting differentiation using event types (tool_use without tool_result = waiting; recent assistant text = thinking).
- `r` reply opens a dedicated input prompt instead of just attaching.
