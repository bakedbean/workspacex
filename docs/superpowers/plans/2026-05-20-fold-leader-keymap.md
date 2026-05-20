# Dashboard fold leader (`z` chord) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the dashboard's single-press `z` into a vim-fold-flavored leader chord: `zz` = toggle focused repo (== today's `z`), `za` = expand all repos, `zM` = fold all repos.

**Architecture:** Add a `z_leader_pending: bool` to `App` (separate from the existing `Ctrl-x`-bound `leader_pending`). On the dashboard view, `z` arms the flag instead of acting immediately; the next keypress consumes the flag and dispatches. Extracts today's inline `z`-toggle logic into a helper so it can be reused for the `zz` branch. Adds two trivial all-repos helpers. Updates the footer hint to advertise the chord.

**Tech Stack:** Rust, crossterm key events, ratatui dashboard.

**Spec:** [`docs/superpowers/specs/2026-05-20-fold-leader-keymap-design.md`](../specs/2026-05-20-fold-leader-keymap-design.md)

---

## File map

- **Modify:** `src/app.rs`
  - `App` struct: add `pub z_leader_pending: bool`
  - `App::new`: init to `false`
  - `handle_key_dashboard`: add chord-dispatch branch at the top (after the existing PM-focus / filter-buffer guards, before the main `match (k.code, k.modifiers)`)
  - Replace the `(KeyCode::Char('z'), _)` arm: arm the flag instead of toggling
  - Add `toggle_focused_fold`, `expand_all_repos`, `fold_all_repos` free functions
  - Add 8 tests in the existing `#[cfg(test)] mod tests` block
- **Modify:** `src/ui/dashboard/layout.rs`
  - Line 100: `("z", "fold")` → `("z", "fold…")`

---

## Task 1: Fold leader chord (single task, TDD)

This is one logical change — a keymap. Everything lands in a single commit so the inter-commit state never has a half-rewired chord. Sub-cycles within the task are TDD: failing tests first, then implementation, then verification.

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/dashboard/layout.rs`

### Sub-cycle A: Field + tests (red)

- [ ] **Step 1: Add the `z_leader_pending` field declaration**

In `src/app.rs`, find the `App` struct (around line 194). Add `pub z_leader_pending: bool,` immediately after `pub leader_pending: bool,` (around line 204):

```rust
    pub leader_pending: bool,
    pub z_leader_pending: bool,
```

In `App::new` (around line 272), find the struct literal `let mut app = Self { ... }` (around line 279). Add `z_leader_pending: false,` immediately after `leader_pending: false,` (around line 289):

```rust
            leader_pending: false,
            z_leader_pending: false,
```

This is a no-op behavior-wise; it just makes the field available for the tests below.

- [ ] **Step 2: Add the 8 failing tests**

In `src/app.rs`, find the existing `#[cfg(test)] mod tests` block (it's the large test module with names like `dashboard_down_at_last_entry_wraps_to_first` around line 2946). Append these tests near the end of the module (just before its closing `}`). Two helpers and the tests:

```rust
    /// Test helper: create an App with N repos registered in the store
    /// and loaded into app.repos. Returns the app + repo ids in order.
    /// Uses a unique tmpdir per call so paths don't collide.
    fn make_app_with_n_repos(n: usize) -> (App, Vec<crate::store::RepoId>) {
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..n {
            let path = std::env::temp_dir().join(format!(
                "wsx-fold-test-{}-{}",
                std::process::id(),
                i
            ));
            let id = store
                .add_repo(&path, &format!("repo-{i}"), "")
                .unwrap();
            ids.push(id);
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-fold-test")).unwrap();
        app.refresh().unwrap();
        (app, ids)
    }

    async fn press(app: &mut App, ch: char, mods: KeyModifiers) {
        handle_key_dashboard(app, KeyEvent::new(KeyCode::Char(ch), mods))
            .await
            .unwrap();
    }

    async fn press_key(app: &mut App, code: KeyCode) {
        handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn z_alone_arms_leader_without_action() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        assert!(app.z_leader_pending, "z should arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "z alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn zz_toggles_focused_repo_fold() {
        let (mut app, ids) = make_app_with_n_repos(2);
        // Select the first repo. Make sure selectable is populated first.
        app.refresh().unwrap();
        app.dashboard.selected = 0;
        let rid = ids[0];
        let key = rid.0 as u64;
        let before = app.dashboard.folded.get(&key).copied();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'z', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zz");
        let after = app.dashboard.folded.get(&key).copied();
        assert_ne!(
            before, after,
            "zz should change the fold state for the focused repo"
        );
    }

    #[tokio::test]
    async fn za_expands_all_repos() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.refresh().unwrap();
        // Pre-fold one repo explicitly so we can see the "expand all" override.
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'a', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after za");
        for id in &ids {
            let key = id.0 as u64;
            assert_eq!(
                app.dashboard.folded.get(&key).copied(),
                Some(false),
                "za should set repo {key} to expanded (false)"
            );
        }
    }

    #[tokio::test]
    async fn z_shift_m_folds_all_repos() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.refresh().unwrap();
        // Pre-expand one repo explicitly so we can see the "fold all" override.
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'M', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "leader should clear after zM");
        for id in &ids {
            let key = id.0 as u64;
            assert_eq!(
                app.dashboard.folded.get(&key).copied(),
                Some(true),
                "zM should set repo {key} to folded (true)"
            );
        }
    }

    #[tokio::test]
    async fn z_then_unknown_clears_leader_without_action() {
        let (mut app, _) = make_app_with_n_repos(2);
        app.refresh().unwrap();
        let selected_before = app.dashboard.selected;
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'x', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after unknown key");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "unknown follow-up should leave fold state unchanged"
        );
        assert_eq!(
            app.dashboard.selected, selected_before,
            "unknown follow-up should be eaten, not pass through to selection"
        );
    }

    #[tokio::test]
    async fn z_then_esc_clears_leader() {
        let (mut app, _) = make_app_with_n_repos(2);
        app.refresh().unwrap();
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press_key(&mut app, KeyCode::Esc).await;
        assert!(!app.z_leader_pending, "Esc should clear the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "Esc should not change fold state"
        );
    }

    #[tokio::test]
    async fn a_alone_is_no_op_on_dashboard() {
        let (mut app, _) = make_app_with_n_repos(2);
        app.refresh().unwrap();
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'a', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "a alone should not arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "a alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn shift_m_alone_is_no_op_on_dashboard() {
        let (mut app, _) = make_app_with_n_repos(2);
        app.refresh().unwrap();
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'M', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "M alone should not arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "M alone should not change fold state"
        );
    }
```

- [ ] **Step 3: Run tests; verify they fail**

```bash
cargo test --lib app::tests::z_alone_arms_leader_without_action 2>&1 | tail -15
cargo test --lib app::tests::zz_toggles_focused_repo_fold 2>&1 | tail -15
cargo test --lib app::tests::za_expands_all_repos 2>&1 | tail -15
cargo test --lib app::tests::z_shift_m_folds_all_repos 2>&1 | tail -15
```

Expected:
- `z_alone_arms_leader_without_action` FAILS (assertion: `z_leader_pending` is false because today's `z` toggles immediately and never sets the flag).
- `zz_toggles_focused_repo_fold` may PASS spuriously (today's `z` toggles, second `z` toggles back — net zero change → `assert_ne!` fails) — confirm it fails.
- `za_expands_all_repos` FAILS (today's `z`+`a` toggles repo then `a` does nothing).
- `z_shift_m_folds_all_repos` FAILS (today's `z`+Shift+M toggles repo then Shift+M does nothing).
- `a_alone_is_no_op_on_dashboard` PASSES today (a is not bound) — that's fine; it's a guard test.
- `shift_m_alone_is_no_op_on_dashboard` PASSES today (M is not bound) — fine.

If the test results don't match expectations, pause and investigate before continuing. (If `zz_toggles_focused_repo_fold` passes today by accident — single `z` toggles to folded, then unknown `z` follow-up gets a second toggle back — confirm by reading the test logic.)

### Sub-cycle B: Implement (green)

- [ ] **Step 4: Add the three helper functions**

In `src/app.rs`, add these three free functions near the top of the file's function definitions (after the existing helpers, e.g. near `current_repo_counts` around line 1702 — placement isn't critical, just keep them adjacent and pub-crate so the tests can reach them indirectly through `handle_key_dashboard`):

```rust
/// Toggle the fold state of the currently focused repo on the
/// dashboard. If a workspace is focused, the repo containing it is
/// the target. Extracted from the original single-key `z` arm so the
/// `zz` chord branch can reuse it.
fn toggle_focused_fold(app: &mut App) {
    let target_rid = match app.selected_target() {
        Some(SelectionTarget::Workspace(wid)) => app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == wid)
            .map(|(rid, _)| *rid),
        Some(SelectionTarget::Repo(rid)) => Some(rid),
        None => None,
    };
    if let Some(rid) = target_rid {
        let id = rid.0 as u64;
        let counts = current_repo_counts(app, rid);
        let currently_expanded = match app.dashboard.folded.get(&id).copied() {
            Some(explicit) => !explicit,
            None => !crate::ui::dashboard::sort::default_fold(counts),
        };
        // Store `true` = folded (i.e. !expanded).
        app.dashboard.folded.insert(id, currently_expanded);
    }
}

/// `za` action: expand every registered repo by inserting an explicit
/// `false` in `dashboard.folded`. Overrides the renderer's
/// default-fold heuristic so even default-folded repos open.
fn expand_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, false);
    }
}

/// `zM` action: fold every registered repo by inserting an explicit
/// `true` in `dashboard.folded`. Overrides the renderer's heuristic.
fn fold_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, true);
    }
}
```

Note on the borrow: `expand_all_repos` and `fold_all_repos` borrow `&app.repos` and mutably borrow `app.dashboard.folded`. This compiles because they're disjoint sub-fields and the borrow checker tracks field-level access through the `&mut App` parameter. If you hit E0502, the fix is to collect `app.repos.iter().map(|r| r.id.0 as u64).collect::<Vec<_>>()` first, then iterate that.

- [ ] **Step 5: Add the chord dispatcher at the top of `handle_key_dashboard`**

In `src/app.rs`, find `handle_key_dashboard` (around line 1341). The function currently has: a PM-focus guard, a Tab handler, a filter-buffer guard, then the main `match (k.code, k.modifiers)` at line 1409. Insert the chord dispatch **immediately before** `match (k.code, k.modifiers)` (around line 1409), like this:

```rust
    // Z-leader chord. When armed by the prior `z` keypress, the next
    // key dispatches and the leader clears unconditionally. Unknown
    // follow-ups are eaten (no fall-through to the main key handler)
    // so accidental `zj` etc. don't move the selection silently.
    if app.z_leader_pending {
        app.z_leader_pending = false;
        match (k.code, k.modifiers) {
            (KeyCode::Char('z'), _) => toggle_focused_fold(app),
            (KeyCode::Char('a'), _) => expand_all_repos(app),
            (KeyCode::Char('M'), m) if m.contains(KeyModifiers::SHIFT) => {
                fold_all_repos(app)
            }
            _ => {} // Esc, unknown key, anything else: just clear.
        }
        return Ok(());
    }
    match (k.code, k.modifiers) {
```

- [ ] **Step 6: Change the existing `z` arm to arm the leader**

In `src/app.rs`, find the existing `(KeyCode::Char('z'), _)` arm (around line 1602-1624). Replace the entire arm body with:

```rust
        (KeyCode::Char('z'), _) => {
            app.z_leader_pending = true;
        }
```

The old logic (selected_target lookup, counts, currently_expanded calculation, insert) is now in `toggle_focused_fold` and used by the `zz` chord branch.

- [ ] **Step 7: Run the tests; verify they pass**

```bash
cargo test --lib app::tests::z_alone_arms_leader_without_action 2>&1 | tail -5
cargo test --lib app::tests::zz_toggles_focused_repo_fold 2>&1 | tail -5
cargo test --lib app::tests::za_expands_all_repos 2>&1 | tail -5
cargo test --lib app::tests::z_shift_m_folds_all_repos 2>&1 | tail -5
cargo test --lib app::tests::z_then_unknown_clears_leader_without_action 2>&1 | tail -5
cargo test --lib app::tests::z_then_esc_clears_leader 2>&1 | tail -5
cargo test --lib app::tests::a_alone_is_no_op_on_dashboard 2>&1 | tail -5
cargo test --lib app::tests::shift_m_alone_is_no_op_on_dashboard 2>&1 | tail -5
```

Expected: all 8 tests pass. If any fail, debug before moving on.

Also run the broader app suite to catch any regressions:

```bash
cargo test --lib app::tests 2>&1 | tail -5
```

Expected: prior tests still pass.

### Sub-cycle C: Footer + finalize

- [ ] **Step 8: Update the footer hint**

In `src/ui/dashboard/layout.rs:100`, change:

```rust
        ("z", "fold"),
```

to:

```rust
        ("z", "fold…"),
```

The ellipsis is `U+2026` (single char, not three dots) — copy it verbatim from this plan to avoid typing three periods.

- [ ] **Step 9: Build + run wider tests**

```bash
cargo build 2>&1 | tail -5
cargo test --lib 2>&1 | tail -10
```

Expected: clean build; tests pass (modulo pre-existing flakes in `external::tests::editor_*`, `pty::session::tests::kill_all_*`, `pm::tests::*resume*` — these are environmental, not caused by this change).

If a non-flake test fails, debug. If only flakes fail, re-run them standalone to confirm they pass in isolation:

```bash
cargo test --lib editor_falls_back_to_env  # example
```

- [ ] **Step 10: Run rustfmt**

```bash
cargo fmt --check 2>&1 | head -10
```

If drift:

```bash
cargo fmt
git status --porcelain
```

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(tui): dashboard fold leader (z chord: zz / za / zM)

Turns the dashboard's single-press `z` into a vim-fold-flavored
leader chord:
- `zz` — toggle fold for focused repo (== today's single `z`)
- `za` — expand every repo on the dashboard
- `zM` — fold every repo on the dashboard

`z` alone now arms the leader; the next keypress consumes it.
Unknown follow-ups are eaten (no fall-through to other dashboard
handlers) so accidental `zj` etc. don't move the selection.

Adds `z_leader_pending: bool` on App, separate from the existing
Ctrl-x-bound `leader_pending` so chord families don't collide.
Footer hint updated from "z fold" to "z fold…" to advertise the
chord.
EOF
)"
```

---

## Done. Final verification

```bash
cargo build --release 2>&1 | tail -3
cargo fmt --check 2>&1 | tail -3
cargo test --lib app::tests 2>&1 | tail -3
```

Manual smoke (TUI):

```bash
./target/release/wsx
# Dashboard:
#   - Press `z` — nothing visible should happen (leader is armed but no UI indicator).
#   - Press `z` again — focused repo toggles fold. Same as before.
#   - Press `z`, then `a` — every repo expands.
#   - Press `z`, then Shift+m (`M`) — every repo folds.
#   - Press `z`, then `Esc` — nothing changes.
#   - Press `z`, then `j` (or any unbound key) — nothing changes; selection didn't move.
#   - Footer shows "z fold…" instead of "z fold".
```
