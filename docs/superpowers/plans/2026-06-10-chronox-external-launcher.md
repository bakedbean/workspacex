# External chronox launcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove wsx's built-in "chronology" timeline view and instead launch the external chronox TUI for a workspace via a keybind, following the existing lazygit pattern.

**Architecture:** Part 1 deletes the entire `crate::chronology` feature and every reference to it (input handlers, rendering, modal, app state, config, settings, footer, tests). Part 2 adds an `open_in_chronox` launcher in `src/commands/external.rs`, a `chronox_cmd` setting, and `c` / `Ctrl-x c` keybindings — mirroring lazygit exactly.

**Tech Stack:** Rust, ratatui/crossterm TUI, SQLite-backed `Store` settings, `shlex` for command parsing.

---

## Important context

- **The codebase will not compile mid-removal.** Removing `src/chronology/` breaks every file that references `crate::chronology`. That is expected. Do Part 1's deletions in the order below, then use `cargo build` after the final deletion (Task 6) as the gate. Do not try to keep it green between Part 1 sub-tasks.
- **`sessionx` stays.** It is the JSONL activity parser used by `src/activity/mod.rs`, `src/error.rs`, and `src/pty/session.rs`. Do **not** remove the `sessionx` dependency from `Cargo.toml`. Only the wsx-local `crate::chronology` module is removed.
- **chronox CLI:** `chronox [worktree]` takes the worktree path as its first positional arg, defaulting to cwd. It is a full-screen TUI, so it needs a window-wrapper command, exactly like lazygit.
- **Use the compiler as your guide.** After deleting the module, `cargo build` will list every remaining reference. Line numbers in this plan are from exploration and will drift as you edit — search by symbol name, not line number.

---

## PART 1 — Remove the built-in chronology feature

### Task 1: Delete the chronology module and its config source

**Files:**
- Delete: `src/chronology/` (entire directory: `mod.rs`, `render.rs`, and any submodules like `nav`)
- Delete: `src/config/chronology_source.rs`
- Modify: `src/config/mod.rs`

- [ ] **Step 1: Delete the chronology module directory**

```bash
git rm -r src/chronology
```

- [ ] **Step 2: Delete the chronology config source**

```bash
git rm src/config/chronology_source.rs
```

- [ ] **Step 3: Remove the chronology module declaration from the crate root**

Find where the module is declared (search for `mod chronology` across the crate, typically `src/lib.rs` or `src/main.rs`):

Run: `grep -rn "mod chronology" src`

Delete the `pub mod chronology;` / `mod chronology;` line you find.

- [ ] **Step 4: Remove chronology wiring from `src/config/mod.rs`**

Search for chronology references and remove the module declaration plus any re-exports/glue that adapt `Store`/`Repo` to the chronology `ConfigSource`:

Run: `grep -n "chronology" src/config/mod.rs`

Delete the `mod chronology_source;` line, any `pub use chronology_source::...;`, and the doc comment referencing `crate::chronology::ConfigSource`. Leave the rest of `config/mod.rs` intact.

- [ ] **Step 5: Do NOT build yet** — the crate will not compile until Tasks 2–6 are done. Proceed.

---

### Task 2: Remove chronology input handlers and helpers

**Files:**
- Modify: `src/app/input.rs`

- [ ] **Step 1: Remove the `Ctrl-x c` and `Ctrl-x C` handlers in the attached leader section**

Search: `grep -n "toggle_chronology_visible\|swap_chronology_side" src/app/input.rs`

Delete both match arms (the `KeyCode::Char('c')` and `KeyCode::Char('C')` arms that call `toggle_chronology_visible(app)` and `swap_chronology_side(app)` respectively). This frees the `c` slot for Part 2.

- [ ] **Step 2: Remove the chronology helper functions**

Delete these functions entirely (search each by name):
- `toggle_chronology_visible`
- `swap_chronology_side`
- `focused_chronology_side`
- `open_change_modal`
- `set_change_detail_scroll`
- `toggle_change_detail_view`
- `open_change_in_editor`

Note: `focused_attached_workspace` is a shared helper — check whether anything outside chronology uses it (`grep -n "focused_attached_workspace" src`). If it is used only by the deleted helpers, delete it too; otherwise keep it.

- [ ] **Step 3: Remove the chronology keyboard-nav handler**

Search: `grep -n "chronology_focused\|chronology::nav\|NavAction" src/app/input.rs`

Delete the block guarded by `if app.chronology_focused { ... }` that imports `crate::chronology::nav` and handles j/k/g/G/Enter/Esc, and the arrow-key block that moves focus between the chronology bar and the panes.

- [ ] **Step 4: Remove the chronology mouse handlers**

Search: `grep -n "chronology_bar_rect\|chronology_entry_rects" src/app/input.rs`

Delete the mouse-wheel scroll handler that hit-tests `chronology_bar_rect` and the click handler that hit-tests `chronology_entry_rects` to open the change-detail modal.

- [ ] **Step 5: Remove leftover imports**

Search: `grep -n "chronology\|ChangeDetail\|DiffViewMode\|EditorOpenDecision\|editor_open_decision" src/app/input.rs`

Delete any now-unused `use` lines referencing these. (Some references resolve in Tasks 3–6; that is fine — you are only removing input.rs's own dangling imports here.)

---

### Task 3: Remove chronology rendering

**Files:**
- Modify: `src/app/render.rs`
- Modify: `src/ui/attached.rs`

- [ ] **Step 1: Remove chronology rendering from `src/app/render.rs`**

Search: `grep -n "chronology\|ChronologyDraw\|split_for_chronology\|render_change_detail_modal\|side_cell_to_line\|ChangeDetail" src/app/render.rs`

Delete:
- The per-frame chronology state clearing and the throttled `refresh_chronology` call.
- The block that builds `ChronologyDraw` and calls `split_for_chronology` (the attached pane should use the full area; replace the split result usage with the full area).
- The auto-scroll adjustment for keyboard nav.
- The storing of hit-test rects back into app state.
- `render_change_detail_modal` and the `side_cell_to_line` helper.
- The `Modal::ChangeDetail { .. } =>` arm in the modal-dispatch match.

When removing `split_for_chronology`, make sure the agent pane is rendered into the full area that was previously split. Read the surrounding code to wire the full `Rect` through.

- [ ] **Step 2: Remove the chronology bar from `src/ui/attached.rs`**

Search: `grep -n "Chronology\|chronology\|Side\|split_for_chronology\|render_chronology_bar" src/ui/attached.rs`

Delete:
- `use crate::chronology::Side;` (top of file).
- The `ChronologyDraw` struct.
- The `ChronologyHits` struct (and remove `chronology_entry_rects` / `chronology_visible_entries` fields from any hits/return struct that is shared — adjust callers accordingly).
- `split_for_chronology`.
- `render_chronology_bar`.

If `render_panes` (or equivalent) took a `ChronologyDraw` parameter or returned `ChronologyHits`, update its signature and call site to drop those.

---

### Task 4: Remove chronology state, settings, and footer entry

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/modal.rs`
- Modify: `src/cli.rs`
- Modify: `src/ui/footer.rs`
- Modify: `src/ui/dashboard/tests.rs` (and the settings struct it constructs)

- [ ] **Step 1: Remove chronology fields and methods from `src/app.rs`**

Search: `grep -n "chronology\|change_detail_view\|ChronologyConfig" src/app.rs`

Delete:
- All `chronology_*` struct fields: `chronology`, `chronology_scroll`, `chronology_last_workspace`, `chronology_entry_rects`, `chronology_bar_rect`, `chronology_focused`, `chronology_sel`, `chronology_visible_entries`, `chronology_last_refresh_ms`.
- The `change_detail_view: crate::ui::modal::DiffViewMode` field.
- Their initializers in the `App` constructor / `Default` impl.
- The `refresh_chronology()` method.
- The `RepoSettingField::ChronologyConfig` enum variant and any match arms handling it.

- [ ] **Step 2: Remove the chronology modal from `src/ui/modal.rs`**

Search: `grep -n "ChangeDetail\|DiffViewMode\|chronology\|sessionx" src/ui/modal.rs`

Delete the `Modal::ChangeDetail { .. }` variant and the `DiffViewMode` enum (and its doc comment referencing the sessionx LCS two-column view). Remove any now-unused imports.

- [ ] **Step 2b: Remove DiffViewMode references in render**

Run: `grep -rn "DiffViewMode" src`

If any remain (e.g. helper signatures in `src/chronology/render.rs` are already deleted), remove the stragglers. There should be none outside the deleted code.

- [ ] **Step 3: Remove the setting key from `src/cli.rs`**

In `known_setting_key()`, delete the `| "chronology_config"` arm. (Leave `detail_bar_config` and `usage_graph_window` — they are unrelated features.) Also search the rest of `cli.rs` for `chronology` / `ChronologyConfig` and remove those references.

Run: `grep -n "chronology\|ChronologyConfig" src/cli.rs`

- [ ] **Step 4: Remove the footer legend entry in `src/ui/footer.rs`**

Search: `grep -n "chronolog\|chronox\| c \|Char('c')" src/ui/footer.rs`

Remove the footer item advertising the chronology toggle (`^x c` / `c`). Leave a gap — Part 2 Task 10 adds the chronox entry.

- [ ] **Step 5: Remove the `chronology_config` settings-struct field**

The settings record carries a `chronology_config` field (constructed e.g. at `src/ui/dashboard/tests.rs:26`).

Run: `grep -rn "chronology_config" src`

Find the struct definition (likely in `src/data/store.rs` or a settings module) and delete the `chronology_config` field, its serialization/deserialization, any default, and every construction site (including the test at `src/ui/dashboard/tests.rs:26`). Do **not** write a store migration — existing `chronology_config` rows in the DB are inert and left in place.

---

### Task 5: Remove the now-dead editor-at-line helpers in external.rs

These exist solely to serve the chronology bar's entry clicks and are now unused.

**Files:**
- Modify: `src/commands/external.rs`

- [ ] **Step 1: Confirm they are unused**

Run: `grep -rn "open_in_editor_at\|editor_open_decision\|EditorOpenDecision\|resolve_editor_at_argv\|known_editor_goto\|GotoStyle" src | grep -v "src/commands/external.rs"`

Expected: no output (all callers were removed in Task 2). If anything prints, that caller must be handled before deleting.

- [ ] **Step 2: Delete the dead helpers**

From `src/commands/external.rs`, delete:
- `pub fn open_in_editor_at`
- `pub enum EditorOpenDecision`
- `pub fn editor_open_decision`
- `fn resolve_editor_at_argv`
- `fn known_editor_goto`
- `enum GotoStyle`
- Their associated `#[cfg(test)]` unit tests (search the `mod tests` block for `resolve_editor_at_argv` / `editor_open_decision` / `goto` and remove those `#[test]` fns).

Keep `resolve_editor_cmd`, `open_in_editor`, `resolve_argv`, `spawn_with_path_arg`, `spawn_with_cwd`, and all other launchers.

---

### Task 6: Remove chronology tests, then build and verify Part 1

**Files:**
- Modify: `src/app/input_tests.rs`

- [ ] **Step 1: Remove chronology test modules**

Search: `grep -n "chronology\|change_detail\|ChangeDetail\|ChronologyConfig\|DiffViewMode" src/app/input_tests.rs`

Delete the `change_detail_toggle_tests` and `change_detail_render_tests` modules and any other chronology-specific tests/assertions.

- [ ] **Step 2: Sweep for any remaining references**

Run: `grep -rn "crate::chronology\|chronology_config\|ChangeDetail\|DiffViewMode\|ChronologyConfig\|chronology_focused\|render_chronology_bar\|split_for_chronology" src`

Expected: no output. Fix any stragglers the compiler/grep finds.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles cleanly with no errors and no `unused` warnings related to chronology.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 5: Run clippy and fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: no warnings; fmt makes no or only trivial changes.

- [ ] **Step 6: Commit Part 1**

```bash
git add -A
git commit -m "feat: remove built-in chronology timeline view"
```

---

## PART 2 — Add the external chronox launcher

### Task 7: Add `resolve_chronox_cmd` and `open_in_chronox` (TDD)

**Files:**
- Modify: `src/commands/external.rs`
- Test: `src/commands/external.rs` (the existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Add these to the `mod tests` block in `src/commands/external.rs`:

```rust
#[test]
fn chronox_uses_configured_command() {
    assert_eq!(
        resolve_chronox_cmd(Some("wezterm start -- chronox")).unwrap(),
        "wezterm start -- chronox"
    );
}

#[test]
fn chronox_errors_when_unconfigured() {
    assert!(resolve_chronox_cmd(None).is_err());
    assert!(resolve_chronox_cmd(Some("   ")).is_err());
}

#[test]
fn chronox_appends_path_when_no_placeholder() {
    // Bare command: worktree path is appended as the final argument.
    let argv = resolve_argv(true_path(), &[("path", "/tmp/wt")], Some("/tmp/wt")).unwrap();
    assert_eq!(argv.last().unwrap(), "/tmp/wt");
}

#[test]
fn chronox_substitutes_path_placeholder() {
    let argv = resolve_argv(
        "wezterm start -- chronox {path}",
        &[("path", "/tmp/wt")],
        Some("/tmp/wt"),
    )
    .unwrap();
    assert_eq!(argv, vec!["wezterm", "start", "--", "chronox", "/tmp/wt"]);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib chronox`
Expected: FAIL — `resolve_chronox_cmd` not found.

- [ ] **Step 3: Implement `resolve_chronox_cmd` and `open_in_chronox`**

Add to `src/commands/external.rs`, next to `open_in_lazygit` / `resolve_lazygit_cmd`:

```rust
/// Resolve and launch chronox (or configured equivalent) with cwd=`worktree`.
/// The worktree path is supplied as chronox's positional argument: either via a
/// `{path}` placeholder in the command, or appended when no placeholder is used.
pub fn open_in_chronox(worktree: &Path, configured: Option<&str>) -> Result<()> {
    let cmd = resolve_chronox_cmd(configured)?;
    spawn_with_path_arg(&cmd, worktree)
}

fn resolve_chronox_cmd(configured: Option<&str>) -> Result<String> {
    if let Some(c) = configured {
        if !c.trim().is_empty() {
            return Ok(c.to_string());
        }
    }
    Err(Error::UserInput(
        "no chronox command configured; set `wsx config set chronox_cmd <cmd>` \
         (e.g. `wezterm start -- chronox`) — wsx's own TUI owns the terminal, \
         so chronox needs a wrapper that opens its own window"
            .into(),
    ))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib chronox`
Expected: PASS (all four chronox tests).

- [ ] **Step 5: Commit**

```bash
git add src/commands/external.rs
git commit -m "feat(external): add open_in_chronox launcher"
```

---

### Task 8: Register the `chronox_cmd` setting key

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add the key to `known_setting_key()`**

In `src/cli.rs`, add `chronox_cmd` to the `matches!` arms alongside the other `*_cmd` keys:

```rust
            | "editor_cmd"
            | "terminal_cmd"
            | "diff_cmd"
            | "lazygit_cmd"
            | "chronox_cmd"
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): accept chronox_cmd setting"
```

---

### Task 9: Wire the keybindings (dashboard `c`, attached `Ctrl-x c`)

**Files:**
- Modify: `src/app/input.rs`

- [ ] **Step 1: Add the dashboard `c` handler**

In the dashboard key handler (the `match` containing `(KeyCode::Char('g'), _) => { ... open_in_lazygit ... }`), add a `c` arm modeled on the lazygit arm:

```rust
        (KeyCode::Char('c'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let info = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = info {
                    let cmd = app.store.get_setting("chronox_cmd").ok().flatten();
                    if let Err(e) =
                        crate::commands::external::open_in_chronox(&path, cmd.as_deref())
                    {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
            // 'c' on a Repo header is intentionally a no-op.
        }
```

- [ ] **Step 2: Add the attached `Ctrl-x c` handler**

In the attached leader section (the `match` containing `KeyCode::Char('g') => { ... open_in_lazygit ... }`, where `id` is the attached `target.workspace_id`), add a `c` arm:

```rust
            KeyCode::Char('c') => {
                let path = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = path {
                    let cmd = app.store.get_setting("chronox_cmd").ok().flatten();
                    if let Err(e) =
                        crate::commands::external::open_in_chronox(&path, cmd.as_deref())
                    {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
                return Ok(());
            }
```

Note: confirm the surrounding arms use `return Ok(())` (the lazygit `g` arm does) and match that convention. Confirm `id` is in scope (it is bound from `target.workspace_id` earlier in the attached leader block).

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles cleanly.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(input): launch chronox with c / ctrl-x c"
```

---

### Task 10: Add the footer legend entry

**Files:**
- Modify: `src/ui/footer.rs`

- [ ] **Step 1: Add the chronox footer item**

Read `src/ui/footer.rs` to see how the existing `e`/`t`/`v`/`g` items are declared for the dashboard and attached views (look for where `lazygit`/`g` is listed). Add a parallel entry labeled for chronox — bare `c` in the dashboard footer list and `^x c` (leader-prefixed) in the attached footer list — using the same struct/format as the neighboring items. Example shape (match the actual local type):

```rust
// dashboard footer items: alongside ("g", "lazygit")
("c", "chronox"),
// attached footer items: alongside the leader-prefixed ("g", "lazygit")
("c", "chronox"),
```

- [ ] **Step 2: Build and test**

Run: `cargo build && cargo test`
Expected: compiles; tests pass (including any footer tests — update footer test expectations if they assert the full item list).

- [ ] **Step 3: Commit**

```bash
git add src/ui/footer.rs
git commit -m "feat(footer): advertise chronox keybind"
```

---

## Final verification

- [ ] **Step 1: Full clean build + test + lint**

Run: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all green.

- [ ] **Step 2: No chronology references remain**

Run: `grep -rn "crate::chronology\|chronology_config\|ChangeDetail\|DiffViewMode\|ChronologyConfig" src`
Expected: no output.

- [ ] **Step 3: Manual smoke test (via the `verify` skill or by hand)**

```bash
# Configure a wrapper that opens its own window:
wsx config set chronox_cmd 'wezterm start -- chronox'
```

- Launch wsx, select a workspace on the dashboard, press `c` → chronox opens on that worktree.
- Attach to a workspace, press `Ctrl-x` then `c` → chronox opens on that worktree.
- With `chronox_cmd` unset, press `c` → an error modal explains how to configure it.

---

## Self-review notes (verified against spec)

- **Spec coverage:** Removal of every listed file/symbol is covered (Tasks 1–6); launcher + setting + keybind + footer covered (Tasks 7–10). The `sessionx`-stays and no-migration constraints are called out explicitly.
- **Added beyond spec (justified):** Task 5 removes `open_in_editor_at` / `EditorOpenDecision` / `editor_open_decision` / `resolve_editor_at_argv` / `known_editor_goto` / `GotoStyle`, confirmed during exploration to be used only by chronology — they would otherwise be dead code and trip `-D warnings`.
- **Type consistency:** `open_in_chronox(worktree, configured)` / `resolve_chronox_cmd(configured)` signatures and the `chronox_cmd` setting key are used identically across Tasks 7–10.
