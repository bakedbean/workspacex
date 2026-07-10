# PM Digest Filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Press `/` in the PM digest pane to type a live, case-insensitive filter over workspace names.

**Architecture:** A new `App::pm_filter: Option<String>` buffer (mirroring `dashboard.filter`) flows into `DigestInputs::filter`; `build_digest` drops non-matching cards so the renderer and the input handler — both of which consume `app.build_pm_digest()` — stay in agreement about counts, selection, and Enter-attach. The renderer echoes the needle in the title line; the input handler intercepts editing keys before the pane's single-key bindings.

**Tech Stack:** Rust, ratatui, crossterm; tests via `cargo test` (TestBackend render tests, tokio input tests).

Spec: `docs/superpowers/specs/2026-07-10-pm-digest-filter-design.md`

## Global Constraints

- Matching is against the **workspace name only**, case-insensitive substring.
- `None` and `Some("")` filters must match everything (no-op).
- CI gates rustfmt, clippy, and tests separately: run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before every commit, not just `cargo test`.
- Comment style: comments state constraints the code can't show, matching the file's existing density.

---

### Task 1: Name filtering in `build_digest` + `App.pm_filter` wiring

**Files:**
- Modify: `src/ui/pm_pane.rs` (DigestInputs at :15, `build_digest` at :65, `digest_tests` construction sites at :550, :586, :635, :674, :698)
- Modify: `src/app.rs` (field block near :530, `Default`-ish init near :655, `build_pm_digest` at :956)
- Test: `src/ui/pm_pane.rs` (`digest_tests` module)

**Interfaces:**
- Consumes: existing `DigestInputs`, `build_digest`, `App::build_pm_digest`.
- Produces: `DigestInputs { filter: Option<&'a str>, .. }`; `App::pm_filter: Option<String>`; `build_pm_digest()` passes `self.pm_filter.as_deref()`. Tasks 2 and 3 rely on these exact names.

- [ ] **Step 1: Write the failing tests**

Append to `mod digest_tests` in `src/ui/pm_pane.rs`:

```rust
    #[test]
    fn filter_matches_names_case_insensitively_and_omits_empty_repos() {
        let repos = vec![repo(1, "alpha"), repo(2, "beta")];
        let workspaces = vec![
            ws(1, 1, "auth-refactor", WorkspaceState::Ready),
            ws(2, 1, "docs-pass", WorkspaceState::Ready),
            // repo 2 has no matching workspaces -> omitted entirely
            ws(3, 2, "site-copy", WorkspaceState::Ready),
        ];
        let empty = HashMap::new();
        let inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &empty,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &HashMap::new(),
            filter: Some("AUTH"),
        };
        let digest = build_digest(&inputs);
        assert_eq!(digest.len(), 1);
        assert_eq!(digest[0].repo_name, "alpha");
        let names: Vec<_> = digest[0].cards.iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["auth-refactor"]);
    }

    #[test]
    fn empty_or_absent_filter_is_a_noop() {
        let repos = vec![repo(1, "alpha")];
        let workspaces = vec![
            ws(1, 1, "one", WorkspaceState::Ready),
            ws(2, 1, "two", WorkspaceState::Ready),
        ];
        let empty = HashMap::new();
        let mut inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &empty,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &HashMap::new(),
            filter: None,
        };
        assert_eq!(card_count(&build_digest(&inputs)), 2);
        inputs.filter = Some("");
        assert_eq!(card_count(&build_digest(&inputs)), 2);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib ui::pm_pane::digest_tests 2>&1 | tail -20`
Expected: COMPILE ERROR — `DigestInputs` has no field `filter` (both new tests and the struct literal). That's the failing state; the five existing `digest_tests` literals will also error until Step 3 adds the field.

- [ ] **Step 3: Implement**

In `src/ui/pm_pane.rs`, add to `DigestInputs` (after `last_activity_ms` at :23):

```rust
    /// Live filter needle: cards whose workspace name doesn't contain it
    /// (case-insensitive) are dropped. `None` or `""` matches everything.
    pub filter: Option<&'a str>,
```

In `build_digest` (:65), compute the needle once before the repo loop and extend the per-workspace filter:

```rust
pub fn build_digest(inputs: &DigestInputs) -> Vec<RepoDigest> {
    let needle = inputs
        .filter
        .filter(|f| !f.is_empty())
        .map(|f| f.to_lowercase());
    let mut out = Vec::new();
    for repo in inputs.repos {
        let mut cards: Vec<DigestCard> = inputs
            .workspaces
            .iter()
            .filter(|(rid, w)| {
                *rid == repo.id
                    && w.state == WorkspaceState::Ready
                    && needle
                        .as_ref()
                        .map(|n| w.name.to_lowercase().contains(n))
                        .unwrap_or(true)
            })
```

(rest of the function unchanged — the existing `if cards.is_empty() { continue; }` already omits repos with no matching cards).

Add `filter: None,` to the five existing `DigestInputs` literals in `digest_tests` (in `filters_non_ready_and_empty_repos`, `orders_blocked_then_waiting_then_stalest_first`, `recap_stale_when_activity_newer_than_recap`, `no_recap_with_activity_present_is_not_stale`, `card_at_indexes_across_repos`).

In `src/app.rs`, add the field after `pm_digest_selected` (:530):

```rust
    /// Live PM digest filter buffer. `None` = inactive; `Some(buf)` = filter
    /// mode, matched case-insensitively against workspace names.
    pub pm_filter: Option<String>,
```

Initialize it next to `pm_digest_selected: 0` (near :655):

```rust
            pm_filter: None,
```

In `build_pm_digest` (:956), add to the `DigestInputs` literal:

```rust
            filter: self.pm_filter.as_deref(),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx --lib pm_pane 2>&1 | tail -5`
Expected: all `digest_tests` and `render_tests` PASS (render tests are untouched by this task).
Then run the full suite to catch other `DigestInputs` construction sites: `cargo test -p wsx 2>&1 | tail -5` — expected PASS. If any other file fails to compile on the missing `filter` field, add `filter: None` there (only `app.rs` and `pm_pane.rs` construct it as of this writing; `app.rs` gets the real value above).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/ui/pm_pane.rs src/app.rs
git commit -m "feat(ui): filter PM digest cards by workspace name"
```

---

### Task 2: Renderer — title echo, `/ filter` hint, zero-match placeholder

**Files:**
- Modify: `src/ui/pm_pane.rs` (`render_digest` at :123, `render_title` at :168, `render_tests::draw` at :352 — existing `draw` call sites stay untouched since `draw` keeps its arity and delegates)
- Modify: `src/app/render.rs` (`render_digest` call at :271)
- Test: `src/ui/pm_pane.rs` (`render_tests` module)

**Interfaces:**
- Consumes: `App::pm_filter` from Task 1.
- Produces: `render_digest(f, area, digest, selected, focus, filter: Option<&str>, now_ms, theme)` — the `filter` param is the raw buffer (`Some("")` means "filter mode armed, nothing typed yet" and MUST render the `/` echo; only the digest-building side treats `""` as match-all).

- [ ] **Step 1: Write the failing tests**

In `render_tests`, change the `draw` helper to accept the filter and thread it through:

```rust
    fn draw(digest: &[RepoDigest], selected: usize, focus: PaneFocus) -> String {
        draw_filtered(digest, selected, focus, None)
    }

    fn draw_filtered(
        digest: &[RepoDigest],
        selected: usize,
        focus: PaneFocus,
        filter: Option<&str>,
    ) -> String {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = crate::ui::theme::Theme::default();
        terminal
            .draw(|f| render_digest(f, f.area(), digest, selected, focus, filter, 10_000, &theme))
            .unwrap();
        buffer_text(&terminal)
    }
```

Append the new tests:

```rust
    #[test]
    fn focused_title_advertises_filter_key() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("w")],
        }];
        assert!(draw(&digest, 0, PaneFocus::ProjectManager).contains("/ filter"));
    }

    #[test]
    fn active_filter_echoes_needle_in_title() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("auth-refactor")],
        }];
        let text = draw_filtered(&digest, 0, PaneFocus::ProjectManager, Some("auth"));
        assert!(text.contains("/auth"), "{text}");
        assert!(text.contains("Esc clear"), "{text}");
    }

    #[test]
    fn zero_match_placeholder_differs_from_empty_placeholder() {
        let with_filter = draw_filtered(&[], 0, PaneFocus::ProjectManager, Some("zzz"));
        assert!(with_filter.contains("no matching workspaces"), "{with_filter}");
        let no_filter = draw(&[], 0, PaneFocus::Dashboard);
        assert!(no_filter.contains("no active workspaces"), "{no_filter}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib ui::pm_pane::render_tests 2>&1 | tail -20`
Expected: COMPILE ERROR — `render_digest` takes 7 arguments but 8 were supplied.

- [ ] **Step 3: Implement**

`render_digest` (:123) gains `filter: Option<&str>` between `focus` and `now_ms`, passes it to the title, and branches the empty placeholder:

```rust
pub fn render_digest(
    f: &mut Frame,
    area: Rect,
    digest: &[RepoDigest],
    selected: usize,
    focus: PaneFocus,
    filter: Option<&str>,
    now_ms: i64,
    theme: &Theme,
) {
```

…call `render_title(f, chunks[0], focus, filter, theme);` and replace the empty-lines placeholder text with:

```rust
    if lines.is_empty() {
        let msg = if filter.map(|n| !n.is_empty()).unwrap_or(false) {
            "no matching workspaces"
        } else {
            "no active workspaces"
        };
        f.render_widget(Paragraph::new(msg).style(theme.dim_style()), body);
        return;
    }
```

`render_title` (:168):

```rust
fn render_title(f: &mut Frame, area: Rect, focus: PaneFocus, filter: Option<&str>, theme: &Theme) {
    let label = match (focus, filter) {
        // Filter mode: echo the live needle even while it's still empty,
        // so the `/` press has visible feedback before any typing.
        (PaneFocus::ProjectManager, Some(needle)) => {
            format!("Project Manager [/{needle} · Esc clear · Enter attach]")
        }
        (PaneFocus::ProjectManager, None) => {
            "Project Manager [j/k select · / filter · Enter attach · Esc/Tab back]".to_string()
        }
        (PaneFocus::Dashboard | PaneFocus::DetailBarReply, _) => {
            "Project Manager [Tab to focus · r refresh · p close]".to_string()
        }
    };
    let width = area.width as usize;
    let used = label.chars().count();
    let gap = 2;
    let rule_len = width.saturating_sub(used + gap);
    let mut spans: Vec<Span<'static>> = vec![Span::styled(label, theme.dim_style())];
    if rule_len > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
```

In `src/app/render.rs` (:271), pass the buffer:

```rust
                crate::ui::pm_pane::render_digest(
                    f,
                    pm_area,
                    &digest,
                    selected,
                    app.focus,
                    app.pm_filter.as_deref(),
                    crate::time::now_ms(),
                    &app.theme,
                );
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx --lib pm_pane 2>&1 | tail -5`
Expected: PASS, including the pre-existing `title_hints_differ_by_focus` (its "Enter attach" / "Tab to focus" substrings survive the new hint text).
Then: `cargo test -p wsx 2>&1 | tail -5` — expected PASS (`src/app/render.rs` is the only other `render_digest` caller).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/ui/pm_pane.rs src/app/render.rs
git commit -m "feat(ui): echo the PM digest filter in the title line"
```

---

### Task 3: Input handling — `/` opens, edit keys, Esc clears, close clears; docs

**Files:**
- Modify: `src/app/input.rs` (PM-focus block at :326-360, dashboard `p` toggle at :740-749)
- Modify: `docs/book/src/daily-use/project-manager-pane.md` (Keys table at :53)
- Test: `src/app/input_tests.rs` (`pm_state_tests` module)

**Interfaces:**
- Consumes: `App::pm_filter` (Task 1), `crate::ui::pm_pane::card_count`, test helpers `press_key` (input_tests.rs:3858) and `test_app_with_two_ready_workspaces` (input_tests.rs:110, workspaces named `first`/`second`).
- Produces: final user-facing behavior; nothing downstream.

- [ ] **Step 1: Write the failing tests**

Append to `mod pm_state_tests` in `src/app/input_tests.rs`:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slash_enters_filter_mode_and_chars_edit_buffer() {
        let mut app = test_app_with_two_ready_workspaces();
        press_key(&mut app, KeyCode::Char('p')).await;
        assert_eq!(app.pm_filter, None);
        press_key(&mut app, KeyCode::Char('/')).await;
        assert_eq!(app.pm_filter.as_deref(), Some(""));
        press_key(&mut app, KeyCode::Char('f')).await;
        press_key(&mut app, KeyCode::Char('i')).await;
        assert_eq!(app.pm_filter.as_deref(), Some("fi"));
        press_key(&mut app, KeyCode::Backspace).await;
        assert_eq!(app.pm_filter.as_deref(), Some("f"));
        // Bound letters become filter text while typing: q must NOT close.
        press_key(&mut app, KeyCode::Char('q')).await;
        assert_eq!(app.pm_filter.as_deref(), Some("fq"));
        assert!(app.pm_visible, "q while filtering edits the buffer");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn filter_esc_clears_then_second_esc_unfocuses() {
        let mut app = test_app_with_two_ready_workspaces();
        press_key(&mut app, KeyCode::Char('p')).await;
        press_key(&mut app, KeyCode::Char('/')).await;
        press_key(&mut app, KeyCode::Char('x')).await;
        press_key(&mut app, KeyCode::Esc).await;
        assert_eq!(app.pm_filter, None, "first Esc clears the filter");
        assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
        press_key(&mut app, KeyCode::Esc).await;
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert!(app.pm_visible, "second Esc only unfocuses");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn filter_edits_clamp_selection_to_filtered_count() {
        let mut app = test_app_with_two_ready_workspaces();
        press_key(&mut app, KeyCode::Char('p')).await;
        press_key(&mut app, KeyCode::Char('j')).await;
        assert_eq!(app.pm_digest_selected, 1);
        press_key(&mut app, KeyCode::Char('/')).await;
        // "first" matches only one card -> selection clamps to 0.
        for c in "first".chars() {
            press_key(&mut app, KeyCode::Char(c)).await;
        }
        assert_eq!(app.pm_digest_selected, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closing_the_pane_clears_the_filter() {
        let mut app = test_app_with_two_ready_workspaces();
        press_key(&mut app, KeyCode::Char('p')).await;
        press_key(&mut app, KeyCode::Char('/')).await;
        press_key(&mut app, KeyCode::Char('x')).await;
        // Tab away (filter persists while the pane stays open), then close
        // from dashboard focus.
        press_key(&mut app, KeyCode::Tab).await;
        assert_eq!(app.pm_filter.as_deref(), Some("x"));
        press_key(&mut app, KeyCode::Char('p')).await;
        assert!(!app.pm_visible);
        assert_eq!(app.pm_filter, None, "closing clears the filter");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib pm_state_tests 2>&1 | tail -20`
Expected: the four new tests FAIL (e.g. `slash_enters_filter_mode…` asserts `Some("")` but gets `None`); pre-existing pm_state_tests still pass.

- [ ] **Step 3: Implement**

In `src/app/input.rs`, inside the PM-focus block (:326), after `app.z_leader_pending = false;` and BEFORE `let digest = app.build_pm_digest();`, insert the edit-key intercept (mirrors the dashboard filter block at :398-422):

```rust
        // Filter editing intercepts printable chars, Backspace, and Esc
        // before the single-key bindings below — while typing, letters
        // like j/k/q/p/r are filter text, not shortcuts. Arrows, Enter,
        // and Tab fall through and keep their meanings.
        if app.pm_filter.is_some() {
            match k.code {
                KeyCode::Esc => {
                    app.pm_filter = None;
                    return Ok(());
                }
                KeyCode::Backspace => {
                    if let Some(buf) = app.pm_filter.as_mut() {
                        buf.pop();
                    }
                    clamp_pm_selection(app);
                    return Ok(());
                }
                KeyCode::Char(c)
                    if !c.is_control()
                        && !k.modifiers.contains(KeyModifiers::CONTROL)
                        && !k.modifiers.contains(KeyModifiers::ALT) =>
                {
                    if let Some(buf) = app.pm_filter.as_mut() {
                        buf.push(c);
                    }
                    clamp_pm_selection(app);
                    return Ok(());
                }
                _ => {}
            }
        }
```

Add `/` to the existing `match k.code` in the same block, and clear the filter in the close arm:

```rust
            KeyCode::Char('q') | KeyCode::Char('p') => {
                app.pm_visible = false;
                app.pm_filter = None;
                app.focus = crate::ui::PaneFocus::Dashboard;
            }
```

```rust
            KeyCode::Char('/') => {
                app.pm_filter = Some(String::new());
            }
```

Add the clamp helper next to `expand_all_repos`/`fold_all_repos` (near :310):

```rust
/// Clamp the PM digest selection after a filter edit shrinks the card list,
/// so the selection marker never points past the visible cards.
fn clamp_pm_selection(app: &mut App) {
    let count = crate::ui::pm_pane::card_count(&app.build_pm_digest());
    app.pm_digest_selected = app.pm_digest_selected.min(count.saturating_sub(1));
}
```

In the dashboard-focused `p` toggle (:740), clear the filter on BOTH branches (closing must drop it; opening must never inherit a stale one):

```rust
        (KeyCode::Char('p'), _) => {
            app.pm_filter = None;
            if app.pm_visible {
                app.pm_visible = false;
                app.focus = crate::ui::PaneFocus::Dashboard;
            } else {
                app.pm_visible = true;
                app.pm_digest_selected = 0;
                app.focus = crate::ui::PaneFocus::ProjectManager;
            }
        }
```

Update the digest-focused Keys table in `docs/book/src/daily-use/project-manager-pane.md` (:55-61) — add one row and adjust the Esc row:

```markdown
| Key (digest focused)   | Action                                            |
| ----------------------- | -------------------------------------------------- |
| `j` / `k` (or arrows)  | Move selection                                    |
| `Enter`                | Attach to the selected workspace                  |
| `/`                    | Filter cards by workspace name (type to narrow)   |
| `Esc` / `Tab`          | Clear the filter (if active) / return focus       |
| `q` / `p`              | Close the digest                                  |
| `r`                    | Force a git/PR cache refresh                      |
```

Note the table says Esc/Tab together for brevity, but only Esc clears the filter; Tab unfocuses and leaves the filter applied. Keep the row wording as written above — "Clear the filter (if active) / return focus" reads correctly for both keys' primary use.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx --lib pm_state_tests 2>&1 | tail -5`
Expected: PASS (all pre-existing + 4 new).
Then the full suite: `cargo test -p wsx 2>&1 | tail -5` — expected PASS (the known-flaky `click_chip_auto_spawns_session_when_missing` may need one retry).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/app/input.rs src/app/input_tests.rs docs/book/src/daily-use/project-manager-pane.md
git commit -m "feat(app): / filter for the PM digest pane"
```
