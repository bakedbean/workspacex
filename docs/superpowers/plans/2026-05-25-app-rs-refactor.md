# `src/app.rs` Refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce `src/app.rs` from ~3463 production lines to under 1000 by extracting five cohesive responsibilities (activity, bell, background, render, input) into sibling modules under `src/app/`, with the `App` struct intact and behavior preserved.

**Architecture:** Pure code motion. Each extracted module sits at `src/app/<name>.rs` and exposes a narrow public API. Functions take `&mut App` (or `&App`); no new abstractions. `pub use` re-exports in `src/app.rs` keep external caller paths (`wsx::app::Foo`) working. The single largest test block (`pm_state_tests`, ~2592 lines) moves to a sibling `src/app/input_tests.rs` file, wired via `#[cfg(test)] #[path = "input_tests.rs"] mod tests;` so it retains private-item access without bloating the production file.

**Tech Stack:** Rust 2024 edition, tokio runtime, ratatui + crossterm for TUI, rusqlite for the store. No new dependencies.

**Spec:** [`docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md`](../specs/2026-05-25-app-rs-refactor-design.md)

---

## Pre-flight: Capture verification baseline

Snapshot the current set of test function names so we can confirm none are lost as code moves. Test paths will change (module prefixes shift), but the leaf function names must remain identical.

- [ ] **Step 1: Run full test suite from a clean main**

```bash
git checkout bakedbean/audit-god-files  # the branch this plan lands on
cargo test --no-fail-fast 2>&1 | tee /tmp/wsx-tests-baseline-raw.txt
```

Expected: tests pass. Note the total count printed at the bottom (e.g., `test result: ok. 312 passed; 0 failed; ...`).

- [ ] **Step 2: Extract leaf test names to a sorted list**

```bash
grep -E '^test ' /tmp/wsx-tests-baseline-raw.txt \
  | awk '{print $2}' \
  | awk -F'::' '{print $NF}' \
  | sort -u > /tmp/wsx-tests-before.txt
wc -l /tmp/wsx-tests-before.txt
```

Expected: a number matching the "passed" count above (give or take ignored/filtered tests). This is the contract for verification at the end.

- [ ] **Step 3: Snapshot current line counts**

```bash
find src -name '*.rs' -not -path '*/target/*' | xargs wc -l | sort -rn | head -20 > /tmp/wsx-loc-before.txt
cat /tmp/wsx-loc-before.txt
```

Expected: `src/app.rs` at the top with ~7018 lines.

---

## Task 1: Scaffold `src/app/` module tree

Create the empty sibling files and declare them as submodules of `app`. After this task, nothing has moved — but the project still compiles, and we have a place to land each subsequent extraction.

**Files:**
- Create: `src/app/activity.rs`
- Create: `src/app/bell.rs`
- Create: `src/app/background.rs`
- Create: `src/app/render.rs`
- Create: `src/app/input.rs`
- Create: `src/app/input_tests.rs`
- Modify: `src/app.rs` (add module declarations near the top)

- [ ] **Step 1: Create the empty submodule files**

```bash
mkdir -p src/app
for f in activity bell background render input input_tests; do
  printf '// %s — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)\n' "$f" > "src/app/$f.rs"
done
```

- [ ] **Step 2: Declare the submodules in `src/app.rs`**

At the very top of `src/app.rs`, immediately after the existing `use` block, add:

```rust
pub mod activity;
pub mod bell;
pub mod background;
pub mod render;
pub mod input;
```

Do NOT add `mod` for `input_tests` here. It is wired from inside `src/app/input.rs` via `#[cfg(test)] #[path = "input_tests.rs"] mod tests;` in Task 6.

- [ ] **Step 3: Verify the project still builds**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Finished ... profile`. There may be warnings about unused modules — that's expected, they're empty.

- [ ] **Step 4: Verify all tests still pass**

```bash
cargo test --no-fail-fast 2>&1 | tail -10
```

Expected: same `passed` count as the pre-flight baseline.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/app/
git commit -m "refactor(app): scaffold src/app/ submodule tree

Adds empty sibling files (activity, bell, background, render, input,
input_tests) under src/app/ and declares them as submodules. No code
moves yet — this just creates the destinations for the extraction
commits that follow."
```

---

## Task 2: Extract `activity.rs`

The smallest, cleanest extraction. The activity classifier is pure (no `App` field access), so the move is purely textual.

**Files:**
- Modify: `src/app/activity.rs` (target — was empty after Task 1)
- Modify: `src/app.rs` (source — remove the moved symbols, add a re-export)

**Symbols to move (locate by name in `src/app.rs`):**
- `pub enum ActivityState` and its full `impl ActivityState { ... }` block
- `fn classify_activity(secs: Option<u64>) -> ActivityState`
- `fn classify_activity_with_events(...)`
- `mod activity_classifier_tests { ... }` (the entire `#[cfg(test)] mod` block)

- [ ] **Step 1: Cut the four symbols from `src/app.rs`**

Use search (e.g. `rg -n '^(pub )?(enum ActivityState|impl ActivityState|fn classify_activity|mod activity_classifier_tests)' src/app.rs`) to find each. Cut each symbol with all of its body (matched braces) into the clipboard.

- [ ] **Step 2: Paste into `src/app/activity.rs`**

Open `src/app/activity.rs` and paste the four symbols in this order: `enum`, `impl`, `classify_activity`, `classify_activity_with_events`, then the `mod activity_classifier_tests` block.

Change visibility: `fn classify_activity` and `fn classify_activity_with_events` must become `pub fn ...` (they were file-private before; now they need to be reachable from `app.rs`'s callers).

- [ ] **Step 3: Add `use` statements at the top of `src/app/activity.rs`**

The activity functions reference `crate::events::WorkspaceEvents` (used by `classify_activity_with_events`). Start with:

```rust
use crate::events::WorkspaceEvents;
```

Run `cargo build` — the compiler will tell you if more `use`s are missing. Add each as the error directs (most likely: nothing else needed, since the classifier is pure).

- [ ] **Step 4: Add a re-export at the top of `src/app.rs`**

Right after `pub mod activity;` (added in Task 1), add:

```rust
pub use crate::app::activity::{ActivityState, classify_activity, classify_activity_with_events};
```

This preserves existing `app::ActivityState` (or just `ActivityState` via `use super::*;` in test modules) call sites within `app.rs` and elsewhere.

- [ ] **Step 5: Build and fix any compile errors**

```bash
cargo build 2>&1 | tail -20
```

Common issues and fixes:
- `error[E0603]: function ... is private` → mark the function `pub` in `activity.rs`.
- `error[E0432]: unresolved import` → add a missing `use` in `activity.rs` per the compiler's hint.
- A test in `app.rs` still references `ActivityState` by relative path → leave it; the re-export covers it.

- [ ] **Step 6: Run the activity tests**

```bash
cargo test activity_classifier 2>&1 | tail -20
```

Expected: every test that previously had `activity_classifier_tests::` in its path now has `app::activity::activity_classifier_tests::` — and they all pass.

- [ ] **Step 7: Run the full test suite to catch regressions**

```bash
cargo test --no-fail-fast 2>&1 | tail -5
```

Expected: same `passed` count as the pre-flight baseline.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/app/activity.rs
git commit -m "refactor(app): extract activity classification to app/activity.rs

Moves ActivityState, classify_activity, classify_activity_with_events,
and the activity_classifier_tests module out of src/app.rs. Pure functions
with no App-state coupling. Re-exported from app.rs so external call
sites keep working."
```

---

## Task 3: Extract `bell.rs`

**Files:**
- Modify: `src/app/bell.rs` (target)
- Modify: `src/app.rs` (source)

**Symbols to move:**
- `enum BellPattern` and its `impl BellPattern { ... }`
- `fn alert_decision(...)`
- `fn bell_pattern_for(state: ActivityState, store: &crate::store::Store) -> BellPattern`
- `fn fire_bell(state: ActivityState, store: &crate::store::Store)`
- `mod bell_tests { ... }`

- [ ] **Step 1: Cut the five symbols from `src/app.rs`**

Locate each with `rg -n '^(pub )?(enum BellPattern|impl BellPattern|fn alert_decision|fn bell_pattern_for|fn fire_bell|mod bell_tests)' src/app.rs`. Cut with bodies.

- [ ] **Step 2: Paste into `src/app/bell.rs`**

In order: `enum`, `impl`, `alert_decision`, `bell_pattern_for`, `fire_bell`, then `mod bell_tests`.

Visibility: `alert_decision`, `bell_pattern_for`, `fire_bell` must become `pub fn ...` (they're called from `app.rs`'s draw/event paths).

- [ ] **Step 3: Add `use` statements at the top of `src/app/bell.rs`**

```rust
use crate::app::activity::ActivityState;
use crate::store::Store;
use std::io::Write;
```

(The `Write` import is for the `\x07` write in `fire_bell`.) If `bell_pattern_for` uses `store.notifications_enabled()` or similar, the `Store` import is sufficient — methods come via the `impl`.

- [ ] **Step 4: Re-export from `src/app.rs`**

Add near the other re-exports:

```rust
pub use crate::app::bell::{BellPattern, alert_decision, fire_bell};
```

(`bell_pattern_for` is an internal helper of `bell.rs`; only `fire_bell` and `alert_decision` are called externally.)

- [ ] **Step 5: Build and fix any compile errors**

```bash
cargo build 2>&1 | tail -20
```

Likely fix: `bell.rs` references `ActivityState` — make sure the `use crate::app::activity::ActivityState;` import is at the top. The internal `mod bell_tests` may need its own `use super::*;` already there from before — leave it.

- [ ] **Step 6: Run the bell tests**

```bash
cargo test bell_tests 2>&1 | tail -20
```

Expected: all `bell_tests::*` tests pass under their new `app::bell::bell_tests::` prefix.

- [ ] **Step 7: Run the full test suite**

```bash
cargo test --no-fail-fast 2>&1 | tail -5
```

Expected: same `passed` count as baseline.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/app/bell.rs
git commit -m "refactor(app): extract bell/alert logic to app/bell.rs

Moves BellPattern, alert_decision, bell_pattern_for, fire_bell, and the
bell_tests module out of src/app.rs. Re-exported from app.rs."
```

---

## Task 4: Extract `background.rs`

The async polling tasks. These already operate on `SharedApp = Arc<Mutex<App>>`, so signatures don't change.

**Files:**
- Modify: `src/app/background.rs` (target)
- Modify: `src/app.rs` (source)

**Symbols to move:**
- `pub async fn tail_workspace_events(...)` (~165 lines)
- `pub async fn branch_drift_poll(app: SharedApp)` (~177 lines)
- `mod external_change_polling_tests { ... }`

Helper functions used only by these two (look for any small `fn` defined near them in `app.rs` that no other top-level function references) move along too. As of the audit there are none — both functions are self-contained — but verify with grep before deciding.

- [ ] **Step 1: Cut the three items from `src/app.rs`**

```bash
rg -n '^(pub )?async fn (tail_workspace_events|branch_drift_poll)' src/app.rs
rg -n '^mod external_change_polling_tests' src/app.rs
```

Cut each with its body.

- [ ] **Step 2: Paste into `src/app/background.rs`**

In order: `tail_workspace_events`, `branch_drift_poll`, then `mod external_change_polling_tests`.

Both functions are already `pub async fn` — preserve that.

- [ ] **Step 3: Add `use` statements**

The moved functions reference a bunch of types. Start with the broad import set and let the compiler trim:

```rust
use crate::app::{App, SharedApp};
use crate::events::{self, WorkspaceEvents};
use crate::git;
use crate::store::{Store, WorkspaceId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, warn};
```

Adjust based on what the compiler reports. The `SharedApp` type alias is defined in `src/app.rs` — if it isn't `pub`, make it `pub type SharedApp = Arc<Mutex<App>>;` there so background.rs can import it. Same for `App` itself (already `pub struct App`).

- [ ] **Step 4: Update `src/app.rs` re-exports**

```rust
pub use crate::app::background::{branch_drift_poll, tail_workspace_events};
```

This is the critical step: `main.rs` does `tokio::spawn(app::branch_drift_poll(app.clone()))` (see `src/main.rs:63`). The re-export keeps that line working unchanged.

- [ ] **Step 5: Build and fix any compile errors**

```bash
cargo build 2>&1 | tail -30
```

Likely fixes:
- `SharedApp` not `pub` → mark `pub type SharedApp = ...;` in `app.rs`.
- Helper functions referenced by tail_workspace_events that live in `app.rs` (e.g., classifier callbacks): either move them too, or mark them `pub(crate)` in `app.rs` and import via `crate::app::helper_name`.

Iterate until clean.

- [ ] **Step 6: Run the polling tests**

```bash
cargo test external_change_polling 2>&1 | tail -20
```

Expected: all tests in that module pass.

- [ ] **Step 7: Run the full test suite plus integration tests**

```bash
cargo test --no-fail-fast 2>&1 | tail -10
```

Expected: same passing count as baseline. Integration tests in `tests/branch_drift.rs` and `tests/smoke.rs` should keep working because `wsx::app::branch_drift_poll` still resolves.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/app/background.rs
git commit -m "refactor(app): extract background polling to app/background.rs

Moves tail_workspace_events, branch_drift_poll, and the
external_change_polling_tests module out of src/app.rs. Re-exported
from app.rs so main.rs and integration tests resolve unchanged."
```

---

## Task 5: Extract `render.rs`

The drawing layer. `draw` is large (~540 lines) but self-contained.

**Files:**
- Modify: `src/app/render.rs` (target)
- Modify: `src/app.rs` (source)

**Symbols to move:**
- `fn draw(f: &mut ratatui::Frame, app: &mut App)` — make `pub fn draw`
- `pub fn draw_for_test(f: &mut ratatui::Frame, app: &mut App)` — already `pub`
- `fn resolve_dashboard_detail_cfg(app: &App) -> DetailBarConfig`
- `fn dashboard_regions(...)`
- `fn nerd_fonts_enabled(store: &Store)`
- `fn pm_enabled(store: &Store)`
- `fn notifications_enabled(store: &Store)`
- `fn read_column_widths(store: &Store) -> ColumnWidths`
- `fn compute_attention_line(...)`
- `fn translate_activity(a: ActivityState) -> ui::updates_bar::ActivityState`
- `mod layout_indicator_cache_tests { ... }`

- [ ] **Step 1: Cut the eleven items from `src/app.rs`**

```bash
rg -n '^(pub )?fn (draw|draw_for_test|resolve_dashboard_detail_cfg|dashboard_regions|nerd_fonts_enabled|pm_enabled|notifications_enabled|read_column_widths|compute_attention_line|translate_activity)\b' src/app.rs
rg -n '^mod layout_indicator_cache_tests' src/app.rs
```

Cut each. Make `draw` public during the cut (it's currently private but is called from the run loop in `app.rs`, which will now need `app::render::draw`).

- [ ] **Step 2: Paste into `src/app/render.rs`**

Recommended order: small helpers first (`*_enabled`, `read_column_widths`, `resolve_dashboard_detail_cfg`, `dashboard_regions`, `translate_activity`, `compute_attention_line`), then `draw`, then `draw_for_test`, then the test module.

- [ ] **Step 3: Add `use` statements at the top of `src/app/render.rs`**

Start broad; trim per compiler:

```rust
use crate::app::{App, ActivityState};
use crate::detail_bar_config::DetailBarConfig;
use crate::store::Store;
use crate::ui::{self, dashboard, attached, modal, theme, updates_bar, PaneFocus};
use crate::ui::dashboard::row::ColumnWidths;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
```

(Adjust per the compiler. The current `draw` body references a lot — let `cargo build` drive the import list.)

- [ ] **Step 4: Update the run loop call site in `src/app.rs`**

Find the body of `pub async fn run<B>` and locate where it calls `draw(f, app)`. Change to `crate::app::render::draw(f, app)`, OR add `use crate::app::render::draw;` at the top of `app.rs`.

- [ ] **Step 5: Re-export `draw_for_test` from `src/app.rs`**

Many tests (in `src/ui/dashboard/tests.rs` and the surviving `pm_state_tests`-style tests not yet moved) reference `wsx::app::draw_for_test`. Add:

```rust
pub use crate::app::render::draw_for_test;
```

- [ ] **Step 6: Build and fix any compile errors**

```bash
cargo build 2>&1 | tail -30
```

Common fixes:
- Missing `use` for a ratatui or wsx type → add per compiler hint.
- A private helper inside `app.rs` called by `draw` → make it `pub(crate)` in `app.rs` and import in `render.rs`.

- [ ] **Step 7: Run the layout-indicator tests**

```bash
cargo test layout_indicator_cache 2>&1 | tail -20
```

- [ ] **Step 8: Run the full test suite**

```bash
cargo test --no-fail-fast 2>&1 | tail -5
```

Expected: baseline passing count.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs src/app/render.rs
git commit -m "refactor(app): extract rendering to app/render.rs

Moves draw, draw_for_test, dashboard_regions, the *_enabled helpers,
read_column_widths, compute_attention_line, translate_activity, and
the layout_indicator_cache_tests module out of src/app.rs.
draw_for_test re-exported from app.rs so dashboard tests resolve."
```

---

## Task 6: Extract `input.rs` + `input_tests.rs`

The biggest and last extraction. Combined ~1050 production lines + ~3094 test lines.

**Files:**
- Modify: `src/app/input.rs` (target — production code)
- Modify: `src/app/input_tests.rs` (target — test modules)
- Modify: `src/app.rs` (source)

**Production symbols to move into `src/app/input.rs`:**
- `pub async fn handle_event(app, shared, evt)` — top-level event router
- `async fn dispatch_key(...)`
- `async fn handle_key_dashboard(...)`
- `async fn handle_key_attached(...)`
- `async fn handle_key_attached_pm(...)`
- `async fn handle_key_modal(...)`
- `async fn handle_detail_bar_reply_key(...)`
- `async fn handle_paste(...)`
- `async fn handle_mouse(...)`
- `fn encode_key(k: KeyEvent) -> Vec<u8>`
- `fn encode_key_for_pty(k: &KeyEvent) -> Option<Vec<u8>>`
- `fn paste_char_to_key(c: char) -> KeyEvent`
- `fn scroll_active(app, rows, up)`
- `fn active_session(app)`
- `fn toggle_focused_fold(app)`, `fn set_focused_fold(app, fold)`
- `fn expand_all_repos(app)`, `fn fold_all_repos(app)`
- `fn current_repo_counts(...)`

**Test modules to move into `src/app/input_tests.rs`:**
- `mod pm_state_tests`
- `mod ctrl_x_esc_tests`
- `mod restore_layout_tests`
- `mod detail_bar_focus_tests`

- [ ] **Step 1: Move the production symbols to `src/app/input.rs`**

```bash
rg -n '^(pub )?(async )?fn (handle_event|dispatch_key|handle_key_dashboard|handle_key_attached|handle_key_attached_pm|handle_key_modal|handle_detail_bar_reply_key|handle_paste|handle_mouse|encode_key|encode_key_for_pty|paste_char_to_key|scroll_active|active_session|toggle_focused_fold|set_focused_fold|expand_all_repos|fold_all_repos|current_repo_counts)\b' src/app.rs
```

Cut each with body. Paste into `src/app/input.rs`. Preserve the existing visibility of each — most are private (`fn`), `handle_event` is the one external entry point.

- [ ] **Step 2: Make `handle_event` public**

Change `async fn handle_event(...)` to `pub async fn handle_event(...)`.

- [ ] **Step 3: Add `use` statements to `src/app/input.rs`**

Start with:

```rust
use crate::app::{App, SharedApp, SelectionTarget, PendingEdit, AppEvent};
use crate::error::Result;
use crate::store::WorkspaceId;
use crate::ui::PaneFocus;
use crossterm::event::{Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
```

Compiler will request more. Likely additions: `crate::pty::session`, `crate::ui::modal::Modal`, `crate::ui::AttachedState`, etc.

- [ ] **Step 4: Move the four test modules to `src/app/input_tests.rs`**

```bash
rg -n '^mod (pm_state_tests|ctrl_x_esc_tests|restore_layout_tests|detail_bar_focus_tests)' src/app.rs
```

Cut each entire `#[cfg(test)] mod foo { ... }` block. Paste all four into `src/app/input_tests.rs` in this order: `pm_state_tests`, `ctrl_x_esc_tests`, `restore_layout_tests`, `detail_bar_focus_tests`.

Inside `input_tests.rs`, each test module starts with `use super::*;` — that still works because the `#[path]` mechanism (next step) makes this file *be* a submodule of `app::input`, so `super::*` resolves to the contents of `input.rs`.

- [ ] **Step 5: Wire `input_tests.rs` into `input.rs`**

At the bottom of `src/app/input.rs`, add:

```rust
#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
```

This is the only `#[path]` indirection in the refactor.

- [ ] **Step 6: Update the run-loop call site in `src/app.rs`**

The `pub async fn run<B>` loop calls `handle_event(...)`. Change it to `crate::app::input::handle_event(...)`, OR add `use crate::app::input::handle_event;` at the top of `app.rs`.

- [ ] **Step 7: Build — expect many visibility errors**

```bash
cargo build 2>&1 | tail -50
```

This is the most error-prone task. Likely fixes:
- Helpers in `app.rs` called by input handlers (e.g., `attach_workspace`, `apply_repo_setting`, `restore_attached_state`, `build_spawn_info`, `reconcile_create_result`, `save_layout_for`, `schedule_detach_refresh`, `maybe_mirror_mcp`, `do_pending_edit`, `rescan_processes`) → make each `pub(crate) fn` (or `pub(crate) async fn`) in `app.rs` and import in `input.rs`.
- `SelectionTarget::*` variants and `RepoSettingField::*` referenced by input handlers — these enums must be `pub` (they should already be).
- `PendingEdit` struct — must be `pub`.
- The `App` field accesses (`app.dashboard`, `app.selectable`, etc.) — every field input.rs touches must already be `pub`. The audit listed them as `pub`; if any are not, mark them `pub` in `app.rs`.

Iterate `cargo build` and fix top-to-bottom until clean.

- [ ] **Step 8: Run the four moved test modules**

```bash
cargo test --no-fail-fast 'pm_state_tests::' 2>&1 | tail -10
cargo test --no-fail-fast 'ctrl_x_esc_tests::' 2>&1 | tail -10
cargo test --no-fail-fast 'restore_layout_tests::' 2>&1 | tail -10
cargo test --no-fail-fast 'detail_bar_focus_tests::' 2>&1 | tail -10
```

Expected: each passes the same number of tests as before. (Their paths now include `app::input::tests::` because of the `#[path]` redirect.)

- [ ] **Step 9: Run the full test suite**

```bash
cargo test --no-fail-fast 2>&1 | tail -10
```

Expected: same `passed` count as the pre-flight baseline.

- [ ] **Step 10: Commit**

```bash
git add src/app.rs src/app/input.rs src/app/input_tests.rs
git commit -m "refactor(app): extract input dispatch to app/input.rs

Moves handle_event, dispatch_key, handle_key_* (dashboard, attached,
attached_pm, modal, detail_bar_reply), handle_paste, handle_mouse, key
encoders, and selection helpers out of src/app.rs. The four large
test modules (pm_state, ctrl_x_esc, restore_layout, detail_bar_focus)
move to a sibling src/app/input_tests.rs file wired via #[cfg(test)]
#[path] so they retain private-item access without bloating
src/app/input.rs.

handle_event re-exported from app.rs not required — run() in app.rs
calls it via crate::app::input::handle_event."
```

---

## Task 7: Final sweep and verification

The mechanical work is done. This task confirms the goal was actually met.

- [ ] **Step 1: Format and lint**

```bash
cargo fmt
cargo clippy --all-targets 2>&1 | tail -30
```

Fix any clippy findings introduced by the move (most likely: unused imports left behind in `app.rs`).

- [ ] **Step 2: Verify line-count target**

```bash
find src -name '*.rs' -not -name 'input_tests.rs' -not -path '*/target/*' \
  | xargs wc -l | awk '$1 > 1000 {print}'
```

Expected: no output. Every production file is under 1000 lines.

If `src/app.rs` is over 1000 lines, the spec's Risk #6 has triggered. Apply the mitigation: extract workspace lifecycle helpers (`attach_workspace`, `restore_attached_state`, `build_spawn_info`, `apply_repo_setting`, `reconcile_create_result`, `save_layout_for`, `maybe_mirror_mcp`, `schedule_detach_refresh`, `rescan_processes`) into a new `src/app/workspace_ops.rs` following the pattern of Tasks 2–6. Re-run this step.

- [ ] **Step 3: Verify test parity (the primary contract)**

```bash
cargo test --no-fail-fast 2>&1 | tee /tmp/wsx-tests-after-raw.txt | tail -5
grep -E '^test ' /tmp/wsx-tests-after-raw.txt \
  | awk '{print $2}' \
  | awk -F'::' '{print $NF}' \
  | sort -u > /tmp/wsx-tests-after.txt
diff /tmp/wsx-tests-before.txt /tmp/wsx-tests-after.txt
```

Expected: `diff` produces no output. The set of test function names is byte-identical to the pre-flight baseline. The `passed` count line at the bottom of `cargo test` matches the baseline count.

If `diff` shows differences, investigate immediately — either a test was renamed (revert and redo the move keeping the name) or a test was dropped (likely a missing `#[cfg(test)]` block in `input_tests.rs`).

- [ ] **Step 4: Verify line-count improvement**

```bash
find src -name '*.rs' -not -path '*/target/*' | xargs wc -l | sort -rn | head -10
```

Expected: `src/app/input_tests.rs` may be at or near the top (~3094 lines, expected — test code), but the largest *production* file is now `src/app/input.rs` at ~1050 or `src/pty/session.rs` at 1785 total / 774 prod. `src/app.rs` should be around 1000.

- [ ] **Step 5: TUI smoke test**

Launch wsx and exercise the affected code paths by hand. The `run` skill (if available) automates this; otherwise do it manually:

```bash
cargo run --release
```

In the running TUI, exercise at minimum:
1. **Dashboard navigation** — `j/k` to move selection, `o` to fold/expand a repo, `O`/`Shift-O` to fold/expand all.
2. **Attach/detach** — Enter on a workspace to attach, `Ctrl-X Esc` to detach.
3. **PM pane** — toggle PM visibility, `Tab` to swap focus, `Esc` to return focus to dashboard.
4. **Modal** — open repo-settings modal, edit a field, save.
5. **Bell** — leave a workspace running long enough to trigger an idle-attention bell, confirm `\x07` rings.
6. **Quit** — `q` exits cleanly.

If any of these regress, the test parity check missed something. Bisect on the commit sequence to identify the failing extraction.

- [ ] **Step 6: Final commit (only if the lint/fmt sweep produced changes)**

```bash
git status
# if src/app.rs or src/app/*.rs show modified after fmt/clippy:
git add -u src/
git commit -m "style(app): cargo fmt + clippy cleanup post-extraction"
```

- [ ] **Step 7: Open the PR**

Using the `pull-request` skill (or `gh pr create` directly):

```bash
gh pr create --title "refactor(app): decompose src/app.rs into focused sibling modules" --body "$(cat <<'EOF'
## Summary
- Extracts five cohesive responsibilities from src/app.rs (~3463 production lines) into sibling modules under src/app/: activity, bell, background, render, input.
- src/app.rs drops to ~1000 production lines (App struct + impl, run loop, lifecycle helpers).
- The 2592-line pm_state_tests block moves to a sibling src/app/input_tests.rs file via #[cfg(test)] #[path].
- Pure code motion — no function bodies altered, no new abstractions, no behavior change. App struct unchanged.

Design: docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md
Plan:   docs/superpowers/plans/2026-05-25-app-rs-refactor.md

## Verification
- `diff /tmp/wsx-tests-before.txt /tmp/wsx-tests-after.txt` is empty — set of test function names byte-identical before/after.
- `find src -name '*.rs' -not -name 'input_tests.rs' | xargs wc -l | awk '$1 > 1000'` reports no rows.
- Full `cargo test` passing count matches baseline.

## Test plan
- [ ] cargo build clean
- [ ] cargo test --no-fail-fast passes with baseline count
- [ ] cargo clippy --all-targets clean
- [ ] Manual TUI smoke: dashboard nav, attach/detach, PM toggle, modal, bell
EOF
)"
```

---

## Self-review summary

Coverage:
- **Spec §Goals** — Tasks 2–6 split each named responsibility; Task 7 verifies the <1000 line goal.
- **Spec §Non-goals** — Plan explicitly forbids App-struct decomposition (no task touches App fields except adding `pub` where needed for cross-module access). No function bodies are rewritten — only their location and visibility.
- **Spec §Module-by-module** — One task per module, in the spec's order, with the same symbol list.
- **Spec §Test placement table** — Each test module's destination matches the spec's table; only `input_tests.rs` uses `#[path]` redirection (Task 6 Step 5).
- **Spec §Migration sequence (7 commits)** — One task per commit, same order.
- **Spec §Verification (4 items)** — Mapped to Task 7 Steps 2, 3, 4, 5. Pre-flight Steps 2 and 3 establish the baselines.
- **Spec §Risks** — Risk #6 (app.rs lands over 1000 lines) has a concrete mitigation embedded in Task 7 Step 2.
- **Spec §Open Question #1** (lifecycle helpers placement) — handled in Task 6 Step 7 (helpers stay in app.rs, made `pub(crate)`) and Task 7 Step 2 (mitigation if it pushes over 1000).
