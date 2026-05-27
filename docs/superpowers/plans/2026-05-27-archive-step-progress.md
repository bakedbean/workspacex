# Archive-modal step progress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single "Removing workspace…" spinner in the archive modal with a four-line checklist that shows which of the four phases of `workspace::archive_with_app` is currently running.

**Architecture:** Promote `Modal::ArchiveRunning` from a unit variant to a struct variant carrying an `ArchiveStep` enum and a `script_present` flag. The caller seeds the modal before spawning the archive task; `archive_with_app` advances the `step` field between phases by briefly locking the shared `App` (the same pattern phase 4 already uses for DB + MCP cleanup). The renderer reads the field on each tick and draws the checklist.

**Tech Stack:** Rust, ratatui (TUI), tokio (async runtime + `Mutex`), `cargo test`.

**Spec:** `docs/superpowers/specs/2026-05-27-archive-step-progress-design.md`

---

## File Structure

**Modify:**
- `src/ui/modal.rs` — add `ArchiveStep` enum, change `Modal::ArchiveRunning` to a struct variant, replace renderer arm with `render_archive_steps` helper, add unit tests.
- `src/workspace.rs` — add three `advance_archive_step` calls between phases in `archive_with_app`. New private helper `advance_archive_step(app, next)`.
- `src/app/input.rs` — caller computes `script_present` and seeds the new modal payload.
- `src/app.rs` — update two `matches!` guards in `reconcile_archive_result` to match the struct variant; update two test fixtures.
- `src/app/input_tests.rs` — update three `matches!`/`= Modal::ArchiveRunning` sites, add assertion that initial step is `Script`, add a new test for step advancement.

No new files. No new dependencies.

---

## Task 1: Introduce `ArchiveStep` enum + struct variant + compile-clean every use site

**Why this is one task:** Promoting `Modal::ArchiveRunning` from a unit variant to a struct variant breaks every existing use site (`matches!(..., Modal::ArchiveRunning)` and `Modal::ArchiveRunning` constructions) at the compiler level. Splitting the enum change across tasks would leave intermediate commits with a broken build. We make all use sites compile in one commit, render still functional (single-line spinner kept temporarily), then enrich the render in Task 2.

**Files:**
- Modify: `src/ui/modal.rs:10-43` (Modal enum), `src/ui/modal.rs:108-112` (render arm)
- Modify: `src/app/input.rs:925` (modal seed in `y` handler)
- Modify: `src/app.rs:981`, `src/app.rs:987` (reconcile `matches!` guards)
- Modify: `src/app.rs:1016`, `src/app.rs:1029` (reconcile test fixtures)
- Modify: `src/app/input_tests.rs:2427`, `src/app/input_tests.rs:2503`, `src/app/input_tests.rs:2517` (input test assertions)

### Steps

- [ ] **Step 1.1: Add `ArchiveStep` enum at top of `src/ui/modal.rs`**

Insert just above `pub enum Modal` (around line 10):

```rust
/// Which phase of `workspace::archive_with_app` is currently running.
/// Used by `Modal::ArchiveRunning` to drive the per-step progress UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveStep {
    /// Phase 1: running the repo's archive script (if any).
    Script,
    /// Phase 2: `git worktree remove` — usually the slow one.
    RemoveWorktree,
    /// Phase 3: `git branch -D`.
    DeleteBranch,
    /// Phase 4: sqlite row + MCP entry cleanup.
    Cleanup,
}
```

- [ ] **Step 1.2: Change `Modal::ArchiveRunning` to a struct variant**

In `src/ui/modal.rs`, replace the line `ArchiveRunning,` (currently line 26) with:

```rust
    ArchiveRunning {
        step: ArchiveStep,
        /// Whether the repo has an archive script configured. Drives
        /// whether the Script row renders as in-progress/done or
        /// "(skipped)".
        script_present: bool,
    },
```

- [ ] **Step 1.3: Update the renderer arm — KEEP single-line text for now**

In `src/ui/modal.rs::render` (around line 108), replace:

```rust
        Modal::ArchiveRunning => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let body = format!("  {frame} Removing workspace…");
            ("archive workspace", body)
        }
```

with:

```rust
        Modal::ArchiveRunning { step: _, script_present: _ } => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let body = format!("  {frame} Removing workspace…");
            ("archive workspace", body)
        }
```

(Body unchanged on purpose — Task 2 replaces it. This step is just about making the variant destructure compile.)

- [ ] **Step 1.4: Update the caller in `src/app/input.rs`**

Around line 925, replace:

```rust
                app.modal = Some(Modal::ArchiveRunning);
```

with:

```rust
                let script_present = repo
                    .archive_script
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                app.modal = Some(Modal::ArchiveRunning {
                    step: crate::ui::modal::ArchiveStep::Script,
                    script_present,
                });
```

Note: `repo` is the `Repo` already bound earlier in the handler at `src/app/input.rs:907-923`.

- [ ] **Step 1.5: Update the `Esc` no-op arm in `src/app/input.rs`**

Around line 956, replace:

```rust
        Modal::ArchiveRunning => {
            // Archive is non-cancellable. Swallow all keys until the
            // spawned task completes and reconciles the modal.
        }
```

with:

```rust
        Modal::ArchiveRunning { .. } => {
            // Archive is non-cancellable. Swallow all keys until the
            // spawned task completes and reconciles the modal.
        }
```

- [ ] **Step 1.6: Update the two `matches!` guards in `src/app.rs::reconcile_archive_result`**

At `src/app.rs:981` and `src/app.rs:987`, change:

```rust
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::ArchiveRunning)) {
```

to:

```rust
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::ArchiveRunning { .. })) {
```

(Two occurrences — do both.)

- [ ] **Step 1.7: Update the two reconcile test fixtures in `src/app.rs`**

At `src/app.rs:1016` and `src/app.rs:1029`, change:

```rust
        app.modal = Some(crate::ui::modal::Modal::ArchiveRunning);
```

to:

```rust
        app.modal = Some(crate::ui::modal::Modal::ArchiveRunning {
            step: crate::ui::modal::ArchiveStep::RemoveWorktree,
            script_present: false,
        });
```

(`RemoveWorktree` is a reasonable mid-flight fixture — any variant works since the tests only care about the variant tag.)

- [ ] **Step 1.8: Update the three `matches!` sites in `src/app/input_tests.rs`**

At `src/app/input_tests.rs:2427`, change:

```rust
                matches!(g.modal, Some(Modal::ArchiveRunning)),
```

to:

```rust
                matches!(g.modal, Some(Modal::ArchiveRunning { .. })),
```

At `src/app/input_tests.rs:2503`, change:

```rust
            assert!(matches!(g.modal, Some(Modal::ArchiveRunning)));
```

to:

```rust
            assert!(matches!(g.modal, Some(Modal::ArchiveRunning { .. })));
```

At `src/app/input_tests.rs:2517`, change:

```rust
                matches!(g.modal, Some(Modal::ArchiveRunning)),
```

to:

```rust
                matches!(g.modal, Some(Modal::ArchiveRunning { .. })),
```

- [ ] **Step 1.9: Compile and run the whole test suite**

Run: `cargo test --all`
Expected: PASS. No new tests yet — this step exists to verify the variant refactor didn't break anything.

If `rustc` flags an `ArchiveRunning` site we missed, fix it before continuing. Search for stragglers:

```bash
rg -n "Modal::ArchiveRunning" src/
```

Every match should now use either `{ .. }` (in patterns) or `{ step: ..., script_present: ... }` (in constructions).

- [ ] **Step 1.10: Commit**

```bash
git add src/ui/modal.rs src/app.rs src/app/input.rs src/app/input_tests.rs
git commit -m "refactor(modal): promote ArchiveRunning to struct variant"
```

---

## Task 2: Add `render_archive_steps` helper and switch the renderer to use it

**Files:**
- Modify: `src/ui/modal.rs` (add helper + tests, switch render arm to call it)

### Steps

- [ ] **Step 2.1: Write the failing test for the "all in-progress on Script" rendering**

Append to the `#[cfg(test)] mod` section at the bottom of `src/ui/modal.rs` (or add a new `mod render_archive_steps_tests` block — whichever is consistent with the file's style; the file currently has multiple sibling `mod ..._tests` blocks):

```rust
#[cfg(test)]
mod render_archive_steps_tests {
    use super::*;

    #[test]
    fn step_script_with_script_present_marks_script_in_progress() {
        let body = render_archive_steps(ArchiveStep::Script, true, 0);
        // Spinner frame for tick=0 is '⠋' (from spinner::frame tests).
        assert!(body.contains("⠋ Running archive script"), "body was:\n{body}");
        assert!(body.contains("· Removing worktree"), "body was:\n{body}");
        assert!(body.contains("· Deleting branch"), "body was:\n{body}");
        assert!(body.contains("· Cleaning up registry"), "body was:\n{body}");
    }
}
```

- [ ] **Step 2.2: Run the test to verify it fails**

Run: `cargo test --lib render_archive_steps_tests::step_script_with_script_present_marks_script_in_progress`
Expected: FAIL — compile error, `render_archive_steps` is not defined.

- [ ] **Step 2.3: Add the `render_archive_steps` helper**

Add this function in `src/ui/modal.rs`, just below the `render` function (around line 135, after the closing brace of `pub fn render`):

```rust
/// Render the 4-line body of the `ArchiveRunning` modal.
///
/// Each line is one phase of `workspace::archive_with_app`. The
/// `script_present` flag overrides the Script row's marker to
/// "— (skipped)" regardless of `step`, so a no-script repo never
/// shows the Script row spinning during the brief window where
/// `step == Script` and `run_archive` is returning `Skipped`.
fn render_archive_steps(step: ArchiveStep, script_present: bool, tick: u32) -> String {
    let spinner = crate::ui::dashboard::spinner::frame(tick);

    // Per-row marker: '✓' done, spinner in-progress, '·' pending.
    // The script row gets a special '(skipped)' rendering when there
    // is no script configured.
    let script_line = if !script_present {
        "  — Archive script (skipped)".to_string()
    } else {
        let m = marker_for(step, ArchiveStep::Script, spinner);
        format!("  {m} Running archive script")
    };
    let worktree_line = {
        let m = marker_for(step, ArchiveStep::RemoveWorktree, spinner);
        format!("  {m} Removing worktree…")
    };
    let branch_line = {
        let m = marker_for(step, ArchiveStep::DeleteBranch, spinner);
        format!("  {m} Deleting branch")
    };
    let cleanup_line = {
        let m = marker_for(step, ArchiveStep::Cleanup, spinner);
        format!("  {m} Cleaning up registry")
    };

    format!("{script_line}\n{worktree_line}\n{branch_line}\n{cleanup_line}")
}

/// Pick the marker character for `row` given the currently running `current` step.
fn marker_for(current: ArchiveStep, row: ArchiveStep, spinner: char) -> char {
    use std::cmp::Ordering;
    match step_ordinal(row).cmp(&step_ordinal(current)) {
        Ordering::Less => '✓',
        Ordering::Equal => spinner,
        Ordering::Greater => '·',
    }
}

fn step_ordinal(s: ArchiveStep) -> u8 {
    match s {
        ArchiveStep::Script => 0,
        ArchiveStep::RemoveWorktree => 1,
        ArchiveStep::DeleteBranch => 2,
        ArchiveStep::Cleanup => 3,
    }
}
```

- [ ] **Step 2.4: Run the test to verify it passes**

Run: `cargo test --lib render_archive_steps_tests::step_script_with_script_present_marks_script_in_progress`
Expected: PASS.

- [ ] **Step 2.5: Add the remaining `render_archive_steps` tests**

Inside the `mod render_archive_steps_tests` block, after the first test, add:

```rust
    #[test]
    fn step_remove_worktree_marks_script_done_and_worktree_in_progress() {
        let body = render_archive_steps(ArchiveStep::RemoveWorktree, true, 0);
        assert!(body.contains("✓ Running archive script"), "body was:\n{body}");
        assert!(body.contains("⠋ Removing worktree"), "body was:\n{body}");
        assert!(body.contains("· Deleting branch"), "body was:\n{body}");
        assert!(body.contains("· Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn step_cleanup_marks_everything_but_cleanup_done() {
        let body = render_archive_steps(ArchiveStep::Cleanup, true, 0);
        assert!(body.contains("✓ Running archive script"), "body was:\n{body}");
        assert!(body.contains("✓ Removing worktree"), "body was:\n{body}");
        assert!(body.contains("✓ Deleting branch"), "body was:\n{body}");
        assert!(body.contains("⠋ Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn script_absent_renders_skipped_regardless_of_step() {
        // Even when step is still Script, no-script repos render
        // the Script row as (skipped) — never spinning.
        for step in [
            ArchiveStep::Script,
            ArchiveStep::RemoveWorktree,
            ArchiveStep::DeleteBranch,
            ArchiveStep::Cleanup,
        ] {
            let body = render_archive_steps(step, false, 0);
            assert!(
                body.contains("— Archive script (skipped)"),
                "step={step:?} body was:\n{body}"
            );
            assert!(
                !body.contains("⠋ Running archive script"),
                "script row should never spin when script_present=false; body was:\n{body}"
            );
        }
    }

    #[test]
    fn spinner_frame_varies_with_tick() {
        // The spinner glyph at tick=0 is '⠋'; at tick=8 it's '⠙'.
        // This sanity-checks that render_archive_steps actually
        // threads `tick` through to spinner::frame.
        let body0 = render_archive_steps(ArchiveStep::RemoveWorktree, true, 0);
        let body8 = render_archive_steps(ArchiveStep::RemoveWorktree, true, 8);
        assert!(body0.contains('⠋'));
        assert!(body8.contains('⠙'));
    }
```

- [ ] **Step 2.6: Run the new tests**

Run: `cargo test --lib render_archive_steps_tests`
Expected: All 5 tests PASS.

- [ ] **Step 2.7: Switch the renderer arm to call `render_archive_steps`**

In `src/ui/modal.rs::render` (around line 108), replace:

```rust
        Modal::ArchiveRunning { step: _, script_present: _ } => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let body = format!("  {frame} Removing workspace…");
            ("archive workspace", body)
        }
```

with:

```rust
        Modal::ArchiveRunning { step, script_present } => {
            let body = render_archive_steps(*step, *script_present, tick);
            ("archive workspace", body)
        }
```

- [ ] **Step 2.8: Bump the modal box height so all four lines fit**

In `src/ui/modal.rs::render` (around line 75), find:

```rust
    let rect = centered(area, 60, 12);
```

The 12-row box is sized for the existing single-line bodies. The four-line checklist plus the top/bottom borders and a blank padding row needs ~14 rows. Replace with:

```rust
    let rect = centered(area, 60, 14);
```

Manual verification: the change is purely a sizing tweak; existing modals (`NewWorkspace`, `ConfirmArchive`, `SetupRunning`, `Error`) all fit comfortably in 14 rows.

- [ ] **Step 2.9: Run the full test suite**

Run: `cargo test --all`
Expected: PASS.

- [ ] **Step 2.10: Commit**

```bash
git add src/ui/modal.rs
git commit -m "feat(modal): render archive progress as a 4-step checklist"
```

---

## Task 3: Advance `step` between phases in `archive_with_app`

**Files:**
- Modify: `src/workspace.rs:285-322` (`archive_with_app` + new `advance_archive_step` helper)
- Modify: `src/app/input_tests.rs` (add a test that observes step advancement)

### Steps

- [ ] **Step 3.1: Write the failing test that observes step advancement**

Add this test inside the existing `#[cfg(test)] mod` block in `src/app/input_tests.rs`, alongside `esc_in_archive_running_is_noop` (around line 2531). This test uses the same `sleep 1` archive-script trick to keep the archive running long enough to observe the step.

```rust
    #[tokio::test]
    async fn archive_step_advances_past_script_phase() {
        use crate::ui::modal::{ArchiveStep, Modal};
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // Archive script is `true` (exits 0 immediately) so phase 1 is
        // fast, but `sleep 0.5` keeps phase 1 running long enough that
        // we can witness the initial Script step before it advances.
        // We just need step to advance past Script during the test.
        store
            .set_repo_archive_script(repo_id, Some("sleep 0.5"))
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let created = crate::workspace::create(
            &store,
            &repo,
            Some("doomed"),
            tmp.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let ws_id = created.workspace.id;
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::ConfirmArchive {
                workspace_id: ws_id,
                name: created.workspace.name.clone(),
            });
        }
        // Press 'y'. Initial step should be Script.
        {
            let mut g = app.lock().await;
            let y = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('y'),
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(y)).await.unwrap();
            match &g.modal {
                Some(Modal::ArchiveRunning { step, script_present }) => {
                    assert_eq!(*step, ArchiveStep::Script, "initial step should be Script");
                    assert!(*script_present, "fixture configured an archive script");
                }
                other => panic!("expected ArchiveRunning, got {other:?}"),
            }
        }
        // Wait long enough for phase 1 to finish (sleep 0.5) and step
        // to advance to at least RemoveWorktree. We don't pin to a
        // specific later step because phases 2/3/4 are fast and any
        // of them is acceptable.
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        {
            let g = app.lock().await;
            match &g.modal {
                Some(Modal::ArchiveRunning { step, .. }) => {
                    assert_ne!(
                        *step,
                        ArchiveStep::Script,
                        "step should have advanced past Script after sleep 0.5 archive script"
                    );
                }
                None => {
                    // Archive already finished — also acceptable. The
                    // important behavior is that step is no longer Script.
                }
                other => panic!("unexpected modal: {other:?}"),
            }
        }
        // Let the archive complete.
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        let g = app.lock().await;
        assert!(g.modal.is_none(), "modal should clear once archive finishes");
        assert!(
            g.workspaces.iter().all(|(_, w)| w.id != ws_id),
            "workspace should be archived"
        );
    }
```

- [ ] **Step 3.2: Run the test to verify it fails**

Run: `cargo test --lib archive_step_advances_past_script_phase`
Expected: FAIL. After the 900ms sleep, the modal's `step` is still `Script` because `archive_with_app` never updates it.

- [ ] **Step 3.3: Add `advance_archive_step` helper at top of `src/workspace.rs`**

Add this private helper somewhere in `src/workspace.rs` — directly above `pub async fn archive_with_app` (currently around line 285) is the natural spot.

```rust
/// Advance the `step` field of the `ArchiveRunning` modal, if the
/// modal still belongs to this archive flow. Called between phases of
/// `archive_with_app`. The check guards against a stale archive task
/// updating a modal that was replaced (e.g. by `Modal::Error` or by a
/// second archive flow).
async fn advance_archive_step(
    app: &crate::app::SharedApp,
    next: crate::ui::modal::ArchiveStep,
) {
    let mut g = app.lock().await;
    if let Some(crate::ui::modal::Modal::ArchiveRunning { step, .. }) = &mut g.modal {
        *step = next;
    }
}
```

- [ ] **Step 3.4: Wire `advance_archive_step` into `archive_with_app`**

In `src/workspace.rs::archive_with_app` (currently around line 285-322), insert calls between the existing phases. The diff (existing lines marked unchanged for context):

```rust
pub async fn archive_with_app(
    app: crate::app::SharedApp,
    repo: Repo,
    ws: Workspace,
    opts: ArchiveOpts,
) -> Result<SetupResult> {
    // --- Phase 1 (unlocked, async): run the archive script if any. ---
    let archive_result = setup::run_archive(
        repo.archive_script.as_deref(),
        &repo.path,
        &ws.worktree_path,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await?;

    // NEW: advance modal to RemoveWorktree before starting phase 2.
    advance_archive_step(&app, crate::ui::modal::ArchiveStep::RemoveWorktree).await;

    // --- Phase 2 (unlocked, async): remove the worktree from disk. ---
    if !opts.keep_worktree && ws.worktree_path.exists() {
        git::remove_worktree(&repo.path, &ws.worktree_path).await?;
    }

    // NEW: advance modal to DeleteBranch before starting phase 3.
    advance_archive_step(&app, crate::ui::modal::ArchiveStep::DeleteBranch).await;

    // --- Phase 3 (unlocked, async): delete the branch. Failures here
    //     are non-fatal and intentionally swallowed, matching `archive`. ---
    let _ = git::branch_delete(&repo.path, &ws.branch, opts.force_branch_delete).await;

    // NEW: advance modal to Cleanup before starting phase 4.
    advance_archive_step(&app, crate::ui::modal::ArchiveStep::Cleanup).await;

    // --- Phase 4 (short, locked): delete the store row + clean up MCP. ---
    {
        let g = app.lock().await;
        g.store.delete_workspace(ws.id)?;
        if crate::mcp::enabled(&g.store)
            && let Err(e) = crate::mcp::remove_worktree_entry(&ws.worktree_path)
        {
            tracing::warn!(error = %e, "failed to remove worktree entry from ~/.claude.json");
        }
    }

    Ok(archive_result)
}
```

Important: the three new `advance_archive_step` calls go *between* the four existing phases, not inside them. Do not change phase bodies.

- [ ] **Step 3.5: Run the new test**

Run: `cargo test --lib archive_step_advances_past_script_phase`
Expected: PASS.

- [ ] **Step 3.6: Run the full test suite**

Run: `cargo test --all`
Expected: PASS.

- [ ] **Step 3.7: Commit**

```bash
git add src/workspace.rs src/app/input_tests.rs
git commit -m "feat(archive): advance modal step between archive_with_app phases"
```

---

## Task 4: Strengthen the existing `y_in_confirm_archive_…` test to assert initial step and `script_present`

**Files:**
- Modify: `src/app/input_tests.rs:2375-2447` (existing `y_in_confirm_archive_transitions_to_archive_running_and_spawns_task`)

### Steps

- [ ] **Step 4.1: Replace the `matches!` assertion with a destructure-and-check**

At `src/app/input_tests.rs:2426-2430`, replace:

```rust
            assert!(
                matches!(g.modal, Some(Modal::ArchiveRunning { .. })),
                "modal should transition to ArchiveRunning immediately; got {:?}",
                g.modal
            );
```

with:

```rust
            match &g.modal {
                Some(Modal::ArchiveRunning { step, script_present }) => {
                    assert_eq!(
                        *step,
                        crate::ui::modal::ArchiveStep::Script,
                        "initial step should be Script"
                    );
                    // The fixture repo at this test site has no
                    // archive script configured, so script_present
                    // must be false.
                    assert!(
                        !*script_present,
                        "fixture has no archive script; script_present should be false"
                    );
                }
                other => panic!(
                    "modal should transition to ArchiveRunning immediately; got {other:?}"
                ),
            }
```

- [ ] **Step 4.2: Run the modified test**

Run: `cargo test --lib y_in_confirm_archive_transitions_to_archive_running_and_spawns_task`
Expected: PASS.

- [ ] **Step 4.3: Run the full test suite**

Run: `cargo test --all`
Expected: PASS.

- [ ] **Step 4.4: Commit**

```bash
git add src/app/input_tests.rs
git commit -m "test(archive): assert initial step + script_present in y-handler test"
```

---

## Task 5: End-to-end manual smoke

This is a human-eye verification step, not an automated test. The new behavior is visual and worth seeing in a real terminal before declaring done.

**Files:** none

### Steps

- [ ] **Step 5.1: Build and run wsx against a real repo with `node_modules`**

If you don't already have a heavy worktree:

```bash
cargo build --release
./target/release/wsx
```

Pick (or create) a workspace whose worktree contains a `node_modules` or `target` directory of meaningful size (hundreds of MB). Open the dashboard and archive it.

Expected:
- The modal immediately shows four lines.
- "Running archive script" is either spinning, marked `✓`, or shows `(skipped)` depending on the repo.
- "Removing worktree…" spins for several seconds, then flips to `✓`.
- "Deleting branch" and "Cleaning up registry" flicker through quickly.
- Modal closes once the workspace is gone from the dashboard.

If the spinner appears frozen or steps don't advance, capture details and re-open the task.

- [ ] **Step 5.2: Repeat on a repo *with* a configured archive script**

In repo settings (`R` from the dashboard, or the relevant key in this codebase), set the archive script to something brief like `echo archived`. Archive a workspace.

Expected:
- "Running archive script" spins briefly, then flips to `✓`.
- The Script row does NOT show `(skipped)`.

- [ ] **Step 5.3: Repeat on a repo *without* an archive script**

Ensure the repo's archive script is unset. Archive a workspace.

Expected:
- "Archive script (skipped)" appears immediately with the `—` marker.
- The Script row never spins.
- The other three rows advance as usual.

---

## Self-review

**Spec coverage:** Each requirement in the spec maps to a task —
- `ArchiveStep` enum + `Modal::ArchiveRunning` struct variant → Task 1
- `render_archive_steps` 4-line body + skipped-row override → Task 2
- `script_present` seeded by caller → Task 1 (step 1.4)
- `advance_archive_step` between phases → Task 3
- Stale-modal guard via `if let Some(Modal::ArchiveRunning { .. })` → Task 3 (step 3.3)
- Modal box height bump (12 → 14) → Task 2 (step 2.8)
- Renderer unit tests for every `(step, script_present)` combo → Task 2 (steps 2.5)
- Updated `y_in_confirm_archive_…` test → Task 4
- New slow-archive observation test → Task 3 (step 3.1)
- Skipped-row visual verification → Task 5 (step 5.3)

**Placeholder scan:** No TBDs, no "implement later", no "similar to" cross-references. Every code-changing step shows the code. Every command shows the expected outcome.

**Type consistency:** `ArchiveStep` is defined once in Task 1 (step 1.1) and referenced by its full path (`crate::ui::modal::ArchiveStep`) from `workspace.rs` and by short path (`ArchiveStep`) inside `modal.rs`. `script_present: bool` is used identically in the variant definition (Task 1, step 1.2), the caller seed (Task 1, step 1.4), the renderer signature (Task 2, step 2.3), and the helper guard (Task 3, step 3.3). `advance_archive_step` has the same signature in its declaration (Task 3, step 3.3) and call sites (Task 3, step 3.4).
