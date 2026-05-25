# `src/app.rs` Refactor — Design

## Audit context

A scan of `src/**/*.rs` for files exceeding 1000 lines total found six candidates:

| File | Total lines | Inline `mod tests` starts at | Approx production lines |
|---|---:|---:|---:|
| `src/app.rs` | 7018 | 3464 | **~3463** |
| `src/pty/session.rs` | 1785 | 774 | ~774 |
| `src/ui/dashboard/detail.rs` | 1686 | 767 | ~767 |
| `src/events.rs` | 1653 | 783 | ~783 |
| `src/store.rs` | 1496 | 766 | ~766 |
| `src/cli.rs` | 1412 | 947 | ~947 |

Five of the six look enormous only because of large inline `#[cfg(test)] mod tests { ... }` blocks. Their *production* code is already under 1000 lines.

Only `src/app.rs` is a god file by production-code volume: ~3463 lines of production code in a single file.

**This spec covers `app.rs` only.** The other five are flagged as future cleanup candidates (e.g., peel large inline test blocks to sibling `_tests.rs` files via `#[cfg(test)] #[path = "..."] mod tests;`) but are out of scope here.

## Goals

- Reduce `src/app.rs` from ~3463 production lines to under 1000 production lines.
- Split out cohesive responsibilities — activity classification, bell/alerting, background polling, rendering, input dispatch — into sibling modules under `src/app/`.
- Preserve behavior exactly. The CLI surface, TUI input, dashboard rendering, and bell semantics stay identical.

## Non-goals

- Not decomposing the `App` struct. It stays a single struct with all current fields. Extracted functions take `&mut App` (or `&App`).
- Not changing function bodies. Aside from `use` and visibility adjustments, no production code is rewritten.
- Not refactoring the five near-miss files (`pty/session.rs`, `ui/dashboard/detail.rs`, `events.rs`, `store.rs`, `cli.rs`).
- Not adding new abstractions, traits, or features.

## Target shape

```
src/
├── app.rs                # ~1000 prod lines + mod derive_stopped_kind_tests inline
├── app/
│   ├── activity.rs       # ~80 prod + ~116 inline tests
│   ├── bell.rs           # ~100 prod + ~152 inline tests
│   ├── background.rs     # ~340 prod + ~50 inline tests
│   ├── render.rs         # ~700 prod + ~43 inline tests
│   ├── input.rs          # ~1050 prod + #[path="input_tests.rs"] mod tests
│   └── input_tests.rs    # ~3094 lines: pm_state + ctrl_x_esc + restore_layout + detail_bar_focus tests
└── ...
```

After the refactor, every *production* file in the project is under 1100 lines.

## Module-by-module breakdown

### `src/app/activity.rs` (~80 prod lines)

**Moves in:**
- `enum ActivityState` + its `impl`
- `fn classify_activity(secs: Option<u64>) -> ActivityState`
- `fn classify_activity_with_events(...)`
- `mod activity_classifier_tests` (inline, ~116 lines)

**Public surface:** `ActivityState`, `classify_activity`, `classify_activity_with_events`.

**`App` field touches:** none. Pure functions over arguments.

### `src/app/bell.rs` (~100 prod lines)

**Moves in:**
- `enum BellPattern` + `impl`
- `fn alert_decision(...)`
- `fn bell_pattern_for(state, store)`
- `fn fire_bell(state, store)`
- `mod bell_tests` (inline, ~152 lines)

**Public surface:** `BellPattern`, `alert_decision`, `fire_bell`.

**`App` field touches:** reads `store` only. `fire_bell` writes `\x07` to stdout — not state.

### `src/app/background.rs` (~340 prod lines)

**Moves in:**
- `async fn tail_workspace_events(...)`
- `async fn branch_drift_poll(app: SharedApp)`
- `mod external_change_polling_tests` (inline, ~50 lines)

**Public surface:** `tail_workspace_events`, `branch_drift_poll`. Both spawned as tokio tasks from `main.rs` and from `run` in `app.rs`.

**`App` field touches:** both functions operate on `SharedApp = Arc<Mutex<App>>` and lock to mutate workspace state. No signature changes.

**Caller compatibility:** `main.rs` calls `app::branch_drift_poll(...)`. Re-export from `app.rs` (`pub use crate::app::background::branch_drift_poll;`) to avoid touching `main.rs`.

### `src/app/render.rs` (~700 prod lines)

**Moves in:**
- `fn draw(f, app)` (~540 lines)
- `pub fn draw_for_test(f, app)`
- `fn resolve_dashboard_detail_cfg(app)`
- `fn dashboard_regions(...)`
- `fn nerd_fonts_enabled(store)`, `fn pm_enabled(store)`, `fn notifications_enabled(store)`
- `fn read_column_widths(store)`
- `fn compute_attention_line(...)`
- `fn translate_activity(...)`
- `mod layout_indicator_cache_tests` (inline, ~43 lines)

**Public surface:** `draw`, `draw_for_test`. Helpers stay `pub(crate)` or private.

**`App` field touches:** reads almost everything; takes `&mut App` because `draw` writes back `chip_rects` and `pinned_commands_cache` each tick.

**Caller compatibility:** `draw_for_test` is referenced by tests in `dashboard::tests` and elsewhere as `wsx::app::draw_for_test`. Re-export from `app.rs`.

> **Intentional coupling:** `draw` writes the hit-test caches `chip_rects` and `pinned_commands_cache` for the input layer to consume on the next event. This render→input data flow survives the split — input.rs continues to read those fields from `&App`.

### `src/app/input.rs` (~1050 prod lines)

**Moves in:**
- `async fn handle_event(app, shared, evt)` — top-level event router
- `async fn dispatch_key(...)`
- `async fn handle_key_dashboard(...)` (~371 lines)
- `async fn handle_key_attached(...)` (~216 lines)
- `async fn handle_key_attached_pm(...)`
- `async fn handle_key_modal(...)` (~322 lines)
- `async fn handle_detail_bar_reply_key(...)`
- `async fn handle_paste(...)`, `async fn handle_mouse(...)`
- Key-encoding helpers: `encode_key`, `encode_key_for_pty`, `paste_char_to_key`
- Selection/scroll helpers used only by input: `scroll_active`, `active_session`, `toggle_focused_fold`, `set_focused_fold`, `expand_all_repos`, `fold_all_repos`, `current_repo_counts`

**Public surface:** `handle_event` (called by the run loop). Everything else `pub(crate)` or private.

**`App` field touches:** many — `dashboard`, `selectable`, `modal`, `focus`, `leader_pending`, `pending_edit`, `chip_rects`, `pinned_commands_cache`, `view`, etc. Functions take `&mut App` (and a `SharedApp` clone where they hand off to background tasks).

**Tests:** four test modules attach to input — `pm_state_tests` (~2592), `ctrl_x_esc_tests` (~119), `restore_layout_tests` (~219), `detail_bar_focus_tests` (~164) — totaling ~3094 lines. To keep `input.rs` itself focused, these are wired via:

```rust
#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
```

This Rust idiom keeps the tests in a sibling file (`src/app/input_tests.rs`) but logically inside the `app::input` module, so they retain private-item access. **This is the only place in the refactor that uses `#[path]` redirection** — every other module's test block is small enough to live inline.

### What stays in `src/app.rs` (~1000 prod lines)

- `enum AppEvent`, `enum SelectionTarget`, `enum RepoSettingField`, `struct PendingEdit`, `enum StoppedKind`
- `struct App` and `impl App` (constructor, `refresh`, accessors)
- `pub async fn run<B>` — the main run loop, now calling into `app::input::handle_event` and `app::render::draw`
- `fn derive_stopped_kind` + `mod derive_stopped_kind_tests` (small, ~99 lines tests)
- `async fn do_pending_edit<B>` — TUI suspend/resume around external editor
- Workspace lifecycle helpers (shared between input handlers and `reconcile_create_result`):
  - `attach_workspace`, `restore_attached_state`, `save_layout_for`, `maybe_mirror_mcp`, `schedule_detach_refresh`, `build_spawn_info`, `apply_repo_setting`, `reconcile_create_result`, `rescan_processes`
- `fn now_ms()` and other small utilities
- Re-exports for caller compatibility:
  - `pub use crate::app::background::{branch_drift_poll, tail_workspace_events};`
  - `pub use crate::app::render::draw_for_test;`
  - Any other names referenced as `wsx::app::Foo` from outside this module.

## Test placement, in detail

| Test module (current) | Lines | Destination |
|---|---:|---|
| `mod activity_classifier_tests` | ~116 | `app/activity.rs`, inline |
| `mod pm_state_tests` | ~2592 | `app/input_tests.rs` (via `#[path]`) |
| `mod external_change_polling_tests` | ~50 | `app/background.rs`, inline |
| `mod layout_indicator_cache_tests` | ~43 | `app/render.rs`, inline |
| `mod bell_tests` | ~152 | `app/bell.rs`, inline |
| `mod derive_stopped_kind_tests` | ~99 | stays in `app.rs`, inline |
| `mod ctrl_x_esc_tests` | ~119 | `app/input_tests.rs` |
| `mod restore_layout_tests` | ~219 | `app/input_tests.rs` |
| `mod detail_bar_focus_tests` | ~164 | `app/input_tests.rs` |

**Rule:** tests move with the code they exercise. Where the resulting `mod tests` block is small (<~200 lines), keep it inline. Where it's large (only `input_tests.rs` here), use `#[cfg(test)] #[path = "..."] mod tests;` to keep the production file readable while preserving private access.

**Verification:** every existing test name appears in the post-refactor `cargo test` output. No tests are renamed. See "Verification" below.

## Migration sequence

The work lands as **one bundled PR** containing the commits below. Each commit compiles and passes `cargo test` on its own — reviewers can step through, and `git bisect` works if regressions appear later.

1. **Scaffold.** Add `pub mod activity; pub mod bell; pub mod background; pub mod render; pub mod input;` to `src/app.rs`. Create empty `src/app/{activity,bell,background,render,input,input_tests}.rs`. `cargo build` passes.
2. **Extract `activity.rs`** (zero `App` field access, smallest blast radius). Move types, functions, and `mod activity_classifier_tests`. Add any needed re-exports to `app.rs`. Run `cargo test activity_classifier`.
3. **Extract `bell.rs`.** Move types, functions, and `mod bell_tests`. Run `cargo test bell_tests`.
4. **Extract `background.rs`.** Move `tail_workspace_events`, `branch_drift_poll`, `mod external_change_polling_tests`. Add `pub use` re-exports so `main.rs` compiles unchanged. Run `cargo test external_change_polling`.
5. **Extract `render.rs`.** Move `draw`, `draw_for_test`, region helpers, `*_enabled` helpers, `compute_attention_line`, `translate_activity`, and `mod layout_indicator_cache_tests`. Re-export `draw_for_test` from `app.rs`. Run `cargo test layout_indicator_cache`.
6. **Extract `input.rs` + `input_tests.rs`** (largest, last). Move all `handle_key_*`, `handle_event`, `handle_paste`, `handle_mouse`, `dispatch_key`, key-encoding helpers, and the four test modules. Wire tests via `#[cfg(test)] #[path = "input_tests.rs"] mod tests;`. Run `cargo test ctrl_x_esc pm_state restore_layout detail_bar_focus`.
7. **Final sweep.** `cargo fmt && cargo clippy --all-targets`. Verify line counts. Run full `cargo test`. Launch the TUI for a smoke test.

**Per-commit invariants:**
- `cargo build` and `cargo test` pass.
- No production-code changes beyond `use` and visibility adjustments.
- Test function names are preserved verbatim.

## Verification

1. **Test parity (primary contract).** Before commit 1, snapshot test names:
   ```
   cargo test 2>&1 | grep -E '^test ' | sort > /tmp/wsx-tests-before.txt
   ```
   After commit 7, the same command must produce a byte-identical file:
   ```
   cargo test 2>&1 | grep -E '^test ' | sort > /tmp/wsx-tests-after.txt
   diff /tmp/wsx-tests-before.txt /tmp/wsx-tests-after.txt   # must be empty
   ```
   This catches dropped tests, renamed tests, and `#[cfg]` mismatches that hide tests.
2. **Line-count target.** No production file over 1000 lines:
   ```
   find src -name '*.rs' -not -name 'input_tests.rs' -not -path '*/target/*' \
     | xargs wc -l | awk '$1 > 1000 {print}'
   ```
   Should print no rows. (`input_tests.rs` is exempt — by design.)
3. **TUI smoke test.** Manually launch wsx, create a workspace, attach, detach, toggle PM (Tab focus swap), navigate the dashboard by keyboard, open and dismiss a modal, trigger a bell. Confirms no regression that tests missed.
4. **Diff sanity.** `git log --stat` per commit should show roughly balanced `+`/`-` counts — code motion, not rewriting.

## Risks and mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Visibility errors after extraction (private helpers used cross-module) | High | Compile-driven. Each commit must build; fix `pub(crate)` as needed. No design-time guesswork. |
| `mod tests` name collision | Low | Each test module keeps its specific name (`bell_tests`, `activity_classifier_tests`). The single generic `mod tests;` is the `#[path]` redirect inside `input.rs`. |
| Circular dependency between extracted modules | Low | All five new modules are leaves: they depend on `App`/`store`/`ui` but not on each other. |
| `main.rs` or external test callers break | Low | Re-export `branch_drift_poll`, `tail_workspace_events`, `draw_for_test` from `app.rs`. Caller paths unchanged. |
| Render→input coupling via `chip_rects` / `pinned_commands_cache` | Known and intentional | Both modules continue to access these via `App`. No new contract needed. |
| Merge conflicts with concurrent work on `app.rs` | Medium | Do the refactor on a quiet day; rebase on `origin/main` immediately before final review. |
| `app.rs` lands over 1000 prod lines after extraction | Medium | If true, extract workspace lifecycle helpers (`attach_workspace`, `restore_attached_state`, `build_spawn_info`, `apply_repo_setting`, `reconcile_create_result`) into a new `app/workspace_ops.rs`. |

**Explicit non-risk: behavior change.** The refactor is pure code motion. No function body is altered. The test-parity check (verification #1) proves this.

## Open questions for review

1. **Are the lifecycle helpers** (`attach_workspace`, `restore_attached_state`, etc.) **in the right place?** This spec keeps them in `app.rs` because they're called from both input handlers and `reconcile_create_result` (which is invoked from background processing). An alternative is `app/workspace_ops.rs`. Decision: keep in `app.rs` for now; if the resulting `app.rs` exceeds 1000 prod lines, this is the first thing we peel out.
2. **Re-export surface.** `pub use crate::app::{background, render}::...` keeps `wsx::app::Foo` paths working for external callers. Acceptable, but consider whether some of these should switch to their canonical path (`wsx::app::background::Foo`) in a follow-up.
3. **Future work (not in this spec):** the five near-miss files. Each could shrink to under 1000 total lines by peeling its inline test block to a sibling `_tests.rs` file via `#[cfg(test)] #[path = "..."] mod tests;`. Cheap mechanical change, can be its own small PR.
