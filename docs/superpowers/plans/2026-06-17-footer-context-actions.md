# Footer Context-Actions Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **⚠️ Plan amended after implementation.** Tasks 1–4 below were executed as
> written. Three refinements then came out of testing and are **not** captured
> as task sections here: (a) the overlay is **navigable** and dispatches the
> action keys rather than swallowing them, (b) `?` opens **only when a workspace
> is selected**, and (c) the `? actions` footer pill is **hidden** when no
> workspace is selected. For the authoritative final behavior, read the design
> spec (`docs/superpowers/specs/2026-06-17-footer-context-actions-design.md`),
> which has been reconciled with the shipped code (PR #193). The flagged claims
> below have been corrected inline so they don't mislead, but the step-by-step
> task bodies still describe the original (pre-refinement) approach.

**Goal:** Remove the four workspace-only hints (edit/term/diff/lazygit) from the dashboard footer and surface all workspace-only actions (incl. chronox) in a `?`-triggered modal overlay.

**Architecture:** Add a dataless `Modal::WorkspaceActions` variant that reuses the existing modal system (the `app.modal` dispatch gate routes all keys to `handle_key_modal` + the generic centered/bordered `modal::render` text path). The overlay's key arm makes it a **navigable card**: `Esc`/`?` close; arrow keys forward to the dashboard to move the selection while the card stays open; `e/t/v/g/c`/`Enter` close then fire against the selection. Wire `?` to open it from the dashboard **only when a workspace is selected**. Trim the static footer `keys` array and add a `? actions` hint that is shown **only when a workspace is selected**.

**Tech Stack:** Rust, ratatui, crossterm. Build/test with `cargo`. Formatting gate: rustfmt 1.95.0 via `mise exec rust@1.95.0 -- cargo fmt --all --check`.

---

## File Structure

- `src/ui/modal/mod.rs` — add `Modal::WorkspaceActions` enum variant (`mod.rs:42-103`) and a render arm in the generic `render()` match (`mod.rs:166-244`). No change to `render.rs` dispatch needed: the `other => modal::render(...)` catch-all at `src/app/render.rs:747` already routes unknown text modals here, and `WorkspaceActions` is **not** in the early-return guard list (`mod.rs:154-161`), so it flows into the text-render match.
- `src/app/input.rs` — open arm in `handle_key_dashboard` (near `/` at `input.rs:672`); dismiss arm in `handle_key_modal` (near `Modal::Error` at `input.rs:1237`).
- `src/ui/dashboard/layout.rs` — trim the `keys` array (`layout.rs:98-109`) and update the existing footer test (`layout.rs:222-237`).

Order matters: build the modal first (Task 1) so the enum variant exists, then the input wiring (Task 2) which references it, then the footer (Task 3). Each task compiles and is committable on its own.

---

## Task 1: Add the `Modal::WorkspaceActions` variant and its renderer

**Files:**
- Modify: `src/ui/modal/mod.rs:98-103` (add enum variant)
- Modify: `src/ui/modal/mod.rs:166-244` (add render match arm)

- [ ] **Step 1: Add the enum variant**

In `src/ui/modal/mod.rs`, add a new dataless variant to the `Modal` enum. Insert it immediately after the `UsageWindowPicker { ... }` variant (currently ending at `mod.rs:102`), before the closing `}` of the enum at `mod.rs:103`:

```rust
    /// Static reference card listing the workspace-only actions
    /// (edit/term/diff/lazygit/chronox) that were removed from the footer.
    /// Carries no state — it is dismissed without side effects.
    WorkspaceActions,
```

- [ ] **Step 2: Build — expect a non-exhaustive-match error**

Run: `cargo build`
Expected: FAIL — the `match modal` in `render()` (`mod.rs:166`) and the `match modal` in `handle_key_modal` (`src/app/input.rs:1100`) are now non-exhaustive (`pattern WorkspaceActions not covered`). This confirms the variant is wired into both match sites. Task 1 fixes the render site; Task 2 fixes the input site.

- [ ] **Step 3: Add the render arm**

In `src/ui/modal/mod.rs`, inside the `let (title, body) = match modal { ... }` block (`mod.rs:166-244`), add this arm. Place it just before the `Modal::AgentPicker { .. }` arm (which currently ends the match around `mod.rs:243`):

```rust
        Modal::WorkspaceActions => (
            "workspace actions",
            "These apply to the selected workspace:\n\n  \
             e   edit        t   term\n  \
             v   diff        g   lazygit\n  \
             c   chronox\n\n  \
             ?/Esc  close"
                .to_string(),
        ),
```

This reuses the generic text path: `modal::render` draws it in the shared `centered(area, 60, 14)` → `Clear` → bordered `Block` box (`mod.rs:164-258`) styled with `theme.header_style()` (the non-`Error` branch at `mod.rs:248`). No `render.rs` change is required — `WorkspaceActions` is absent from the early-return guard (`mod.rs:154-161`) and the dispatch catch-all (`render.rs:747`) forwards it here.

- [ ] **Step 4: Build to verify the render site compiles**

Run: `cargo build`
Expected: Still FAILS, but now only on the `handle_key_modal` match in `src/app/input.rs` (`pattern WorkspaceActions not covered`). The `render()` match in `mod.rs` no longer errors. (If `cargo build` reports any error in `mod.rs`, fix it before moving on.)

- [ ] **Step 5: Commit**

The build is intentionally still red here because the input arm lands in Task 2. Commit the render-side change now; do not run a full `cargo build` gate on this commit.

```bash
git add src/ui/modal/mod.rs
git commit -m "feat(modal): add WorkspaceActions overlay variant and renderer"
```

---

## Task 2: Wire open (`?`) and dismiss (Esc/`?`) for the overlay

**Files:**
- Modify: `src/app/input.rs:672` (open arm in `handle_key_dashboard`)
- Modify: `src/app/input.rs:1237` (dismiss arm in `handle_key_modal`)
- Test: `src/app/input.rs` (add a `#[tokio::test]` or extend existing input tests — see Step 5)

- [ ] **Step 1: Add the dismiss arm (fixes the build first)**

In `src/app/input.rs`, in `handle_key_modal`, add a `Modal::WorkspaceActions` arm. Place it immediately after the `Modal::Error { .. }` arm that ends at `input.rs:1241`:

```rust
        Modal::WorkspaceActions => {
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('?')) {
                app.modal = None;
            }
        }
```

> **Amended:** the shipped arm does **not** swallow the action keys. It was
> refined into a navigable card: `Up`/`Down`/`j`/`k` forward to
> `handle_key_dashboard` to move the selection (card stays open), and
> `e`/`t`/`v`/`g`/`c`/`Enter` close the overlay then forward to
> `handle_key_dashboard` so the action fires against the current selection. Only
> `Esc`/`?` close without side effects, and unrecognized keys are inert. See the
> design spec §3 for the final arm.

In the original plan, all other keys fell through and were swallowed by this arm (the modal dispatch gate at `input.rs:2051` prevents keys from reaching dashboard handlers unless this arm forwards them explicitly).

- [ ] **Step 2: Build to verify exhaustiveness is satisfied**

Run: `cargo build`
Expected: PASS (both match sites now cover `WorkspaceActions`). The overlay can be dismissed but not yet opened.

- [ ] **Step 3: Add the open arm**

In `src/app/input.rs`, in `handle_key_dashboard`, add an arm to open the overlay. Place it immediately after the `(KeyCode::Char('/'), _)` filter arm that ends at `input.rs:674`.

> **Amended:** the shipped arm gates the open on a workspace being selected
> (see below). The original plan opened it unconditionally.

```rust
        (KeyCode::Char('?'), _) => {
            if matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
                app.modal = Some(Modal::WorkspaceActions);
            }
        }
```

Note `Modal` and `SelectionTarget` are already in scope in this file (used throughout, e.g. `input.rs:657` and the `e`/`t`/`v` arms at `input.rs:545-592`), so no new `use` is needed.

- [ ] **Step 4: Build to verify the open arm compiles**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 5: Write a test for open + dismiss + input capture**

Add the following test to the `#[cfg(test)] mod tests` block in `src/app/input.rs`. (Search the file for `mod tests` to locate it. Match the construction of `App` used by existing input tests in that module — if they use a helper such as `test_app()` or `App::new_for_test(...)`, reuse it verbatim; the body below assumes a helper named `test_app()` returning an `App` on the dashboard view. Adapt the constructor call to whatever the surrounding tests use, leaving the assertions unchanged.)

```rust
    #[tokio::test]
    async fn question_mark_opens_and_closes_workspace_actions_overlay() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();

        // `?` on the dashboard opens the overlay.
        let open = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
        handle_key_dashboard(&mut app, open).await.unwrap();
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "expected WorkspaceActions modal to be open, got {:?}",
            app.modal
        );

        // Esc closes it.
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let shared = test_shared(&app);
        handle_key_modal(&mut app, &shared, esc).await.unwrap();
        assert!(app.modal.is_none(), "expected overlay dismissed on Esc");
    }
```

If `handle_key_modal` requires a `SharedApp` argument that has no convenient test constructor, drop the Esc half of the test and instead assert dismissal by directly setting `app.modal = Some(Modal::WorkspaceActions)` then calling the same matches-logic path the existing modal tests use; the essential assertion is that `?` opens the overlay. Keep whichever form compiles against the existing test helpers — do not invent a `SharedApp` constructor.

- [ ] **Step 6: Run the test**

Run: `cargo test --lib question_mark_opens_and_closes_workspace_actions_overlay -- --nocapture`
Expected: PASS. (If the test does not compile due to test-helper mismatch, adjust per Step 5's guidance and re-run.)

- [ ] **Step 7: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(dashboard): open workspace-actions overlay with '?'"
```

---

## Task 3: Trim the footer and add the `? actions` hint

**Files:**
- Modify: `src/ui/dashboard/layout.rs:98-109` (the `keys` array)
- Test: `src/ui/dashboard/layout.rs:222-237` (update existing `footer_includes_keybinds_and_sparkline`)

- [ ] **Step 1: Update the failing test first (TDD)**

In `src/ui/dashboard/layout.rs`, the test `footer_includes_keybinds_and_sparkline` currently asserts `assert!(t.contains(" lazygit"));` at `layout.rs:232`. Replace that single line with assertions for the new shape:

```rust
        assert!(t.contains(" actions"), "actions hint present: {t:?}");
        assert!(!t.contains(" lazygit"), "lazygit hint removed: {t:?}");
        assert!(!t.contains(" edit"), "edit hint removed: {t:?}");
        assert!(!t.contains(" term"), "term hint removed: {t:?}");
        assert!(!t.contains(" diff"), "diff hint removed: {t:?}");
```

Leave the surrounding assertions (`↑↓`, ` nav`, ` group`, ` quit`, `24h `, `v0.5.0`) unchanged.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib footer_includes_keybinds_and_sparkline -- --nocapture`
Expected: FAIL — ` actions` is not present and ` lazygit`/` edit`/` term`/` diff` are still present (the `keys` array hasn't changed yet).

- [ ] **Step 3: Edit the `keys` array**

In `src/ui/dashboard/layout.rs`, replace the `keys` array at `layout.rs:98-109` with:

```rust
    let keys = [
        ("↑↓", "nav"),
        ("↵", "open"),
        ("n", "new"),
        ("G", "group"),
        ("/", "filter"),
        ("?", "actions"),
        ("q", "quit"),
    ];
```

This removes `("e","edit")`, `("t","term")`, `("v","diff")`, `("g","lazygit")` and inserts `("?","actions")` just before `quit`. The loop below (`layout.rs:126-158`) and `key_for_glyph` (`src/ui/footer.rs:41-58`) handle the new hint automatically: `?` → `KeyCode::Char('?')`, so the rendered pill is clickable and synthesizes the same key that opens the overlay. The removed entries take their `FooterHintSpan` click regions with them — no orphaned clickable area remains.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib footer_includes_keybinds_and_sparkline -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Run the full layout test module**

Run: `cargo test --lib ui::dashboard::layout`
Expected: PASS (including `footer_key_pill_wraps_key_only_not_label`, which keys off `↑↓` and is unaffected).

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/layout.rs
git commit -m "feat(dashboard): move workspace-only hints out of footer into '?' overlay"
```

---

## Task 4: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Full test suite**

Run: `cargo test`
Expected: PASS. (If any `app::input` PTY-timing tests flake, re-run them in isolation before treating as a regression — they are known-flaky under the full suite.)

- [ ] **Step 2: Clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Format check (pinned toolchain)**

Run: `mise exec rust@1.95.0 -- cargo fmt --all --check`
Expected: no diff. If it reports formatting changes, run `mise exec rust@1.95.0 -- cargo fmt --all` and amend the relevant commit.

- [ ] **Step 4: Manual smoke test in the running TUI**

Run the dashboard (e.g. `cargo run`) and confirm (reflecting the shipped behavior):
- With a **workspace** selected, the footer reads `↑↓ nav  ↵ open  n new  G group  / filter  ? actions  q quit` — no edit/term/diff/lazygit. With a repo header / nothing selected, the `? actions` pill is **absent**.
- With a workspace selected, pressing `?` opens a centered bordered box listing edit/term/diff/lazygit/chronox with `?/Esc  close`. On a repo header / empty selection, `?` is a **no-op**.
- Esc closes it; `?` toggles it closed.
- Clicking the `? actions` footer pill opens the same overlay.
- With the overlay open, `↑↓`/`j`/`k` move the dashboard selection while the card stays open; `e`/`t`/`v`/`g`/`c` and `Enter` fire against the current selection and close the card.

---

## Notes for the implementer

- This is taste/UX work on the `bakedbean/footer-context-actions` branch; keep the three feature commits separate as laid out (modal, input, footer).
- Task 1's commit is intentionally left with a red build (the input arm completes it in Task 2). If your workflow forbids committing a non-compiling tree, fold Task 1 and Task 2 into a single commit at the end of Task 2 instead — the code is unchanged either way.
- Do not modify the `e`/`t`/`v`/`g`/`c` key *handlers* (`src/app/input.rs:545-592`); their behavior is unchanged.
