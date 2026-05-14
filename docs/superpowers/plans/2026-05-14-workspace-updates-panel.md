# Workspace Updates Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface activity from other workspaces while attached to a workspace or attached-pm view — a conditional status row above the footer, plus a floating `Ctrl-a u` panel listing all workspaces grouped by repo.

**Architecture:** A new pure `src/ui/updates_bar.rs` module selects what the status row should show given pre-computed slices of `App` state (no `App` dependency). A new `Modal::UpdatesPanel` variant uses the existing hard-modal pattern but routes its rendering through a new `modal::render_updates_panel` function called directly by `draw()` (so it can take the live `App` slices it needs). `Ctrl-a u` chord opens the panel from `View::Attached(_)` and `View::AttachedPm`.

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.28 — no new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-14-workspace-updates-panel-design.md`.

---

## File Structure

**Create:**
- `src/ui/updates_bar.rs` — `UpdatesRow`, `UpdatesRowKind`, `WorkspaceUpdateInfo`, `select_row`, age formatter.

**Modify:**
- `src/ui/mod.rs` — declare `pub mod updates_bar;`.
- `src/ui/modal.rs` — add `Modal::UpdatesPanel` variant + new `pub fn render_updates_panel(...)`.
- `src/app.rs` —
  - Route `Modal::UpdatesPanel` through `render_updates_panel` in `draw()`.
  - Extend `handle_key_attached` and `handle_key_attached_pm` Ctrl-a chord with `u` arm.
  - Extend `handle_key_modal` with `Modal::UpdatesPanel` arm (Esc closes, others swallowed).
- `src/ui/attached.rs` — 3-chunk Layout (term / optional status row / footer); render `updates_bar::select_row` result above footer when present; update footer hint text.
- `README.md` — keybinding row + new "Workspace updates panel" section.

---

## Task 1: `updates_bar` module + `select_row` pure function

**Files:**
- Create: `src/ui/updates_bar.rs`
- Modify: `src/ui/mod.rs`

Builds the content-selection logic in isolation. Pure function over slices; no `App` dependency.

- [ ] **Step 1: Declare module**

In `src/ui/mod.rs`, add at the top alongside the other `pub mod` declarations:

```rust
pub mod updates_bar;
```

- [ ] **Step 2: Create `src/ui/updates_bar.rs` with skeleton + tests**

```rust
//! Content selection for the attached-view "other workspaces" status row.
//!
//! Pure module: takes pre-computed slices of App state, returns Option<UpdatesRow>.
//! The caller (typically `attached::render`) handles drawing.

use crate::events::WorkspaceEvents;
use crate::store::WorkspaceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatesRowKind {
    Attention,
    Activity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatesRow {
    pub glyph: char,
    pub kind: UpdatesRowKind,
    pub text: String,
}

/// Activity classification mirrors `app::ActivityState`. Kept here as a
/// re-export-friendly enum so updates_bar doesn't depend on app.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Active,
    Idle,
    Waiting,
    Awaiting,
    Off,
}

#[derive(Debug, Clone)]
pub struct WorkspaceUpdateInfo<'a> {
    pub id: WorkspaceId,
    pub name: &'a str,
    pub events: Option<&'a WorkspaceEvents>,
    pub activity: ActivityState,
    pub needs_attention: bool,
    /// `Some((tool_name, first_seen_ms))` when a tool_use has been pending
    /// for the App's stale threshold. Caller computes via
    /// `App::awaiting_permission`.
    pub awaiting_tool: Option<(String, i64)>,
}

const RECENT_EVENT_MS: i64 = 60_000;

pub fn select_row(
    attached_workspace: Option<WorkspaceId>,
    candidates: &[WorkspaceUpdateInfo],
    now_ms: i64,
) -> Option<UpdatesRow> {
    // Attention priority: among candidates with needs_attention == true,
    // excluding the attached workspace, pick the most recently active.
    let mut attention: Vec<&WorkspaceUpdateInfo> = candidates
        .iter()
        .filter(|c| c.needs_attention && Some(c.id) != attached_workspace)
        .collect();
    attention.sort_by_key(|c| {
        // Sort by most-recent first. Prefer awaiting_tool timestamp (when
        // pending) else latest event timestamp else 0.
        let ts = c
            .awaiting_tool
            .as_ref()
            .map(|(_, t)| *t)
            .or_else(|| c.events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms)))
            .unwrap_or(0);
        -ts
    });
    if let Some(c) = attention.first() {
        let (state_summary, age_anchor_ms) = match &c.awaiting_tool {
            Some((tool, ts)) => (format!("awaiting permission: {tool}"), *ts),
            None => {
                let ts = c
                    .events
                    .and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms))
                    .unwrap_or(now_ms);
                ("waiting".to_string(), ts)
            }
        };
        let age = format_age(now_ms.saturating_sub(age_anchor_ms));
        return Some(UpdatesRow {
            glyph: '⚠',
            kind: UpdatesRowKind::Attention,
            text: format!("{} {} ({})", c.name, state_summary, age),
        });
    }

    // Recent event fallback: among candidates (excluding attached) with a
    // latest event newer than RECENT_EVENT_MS, pick the most recent.
    let mut events: Vec<(&WorkspaceUpdateInfo, &crate::events::EventSnapshot)> = candidates
        .iter()
        .filter(|c| Some(c.id) != attached_workspace)
        .filter_map(|c| c.events?.latest.as_ref().map(|e| (c, e)))
        .filter(|(_, e)| now_ms.saturating_sub(e.timestamp_ms) <= RECENT_EVENT_MS)
        .collect();
    events.sort_by_key(|(_, e)| -e.timestamp_ms);
    if let Some((c, e)) = events.first() {
        let age = format_age(now_ms.saturating_sub(e.timestamp_ms));
        return Some(UpdatesRow {
            glyph: '●',
            kind: UpdatesRowKind::Activity,
            text: format!("{}: {} ({})", c.name, e.display, age),
        });
    }

    None
}

/// Format a millisecond delta as `<n>s` for <60s, `<n>m` for <60m, `<n>h` otherwise.
pub fn format_age(delta_ms: i64) -> String {
    let secs = (delta_ms / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventSnapshot, WorkspaceEvents};
    use crate::store::WorkspaceId;

    fn ws(
        id: i64,
        name: &str,
        events: Option<WorkspaceEvents>,
        activity: ActivityState,
        needs_attention: bool,
        awaiting: Option<(String, i64)>,
    ) -> (WorkspaceId, Option<WorkspaceEvents>, ActivityState, bool, Option<(String, i64)>, String) {
        (
            WorkspaceId(id),
            events,
            activity,
            needs_attention,
            awaiting,
            name.to_string(),
        )
    }

    fn snap(display: &str, timestamp_ms: i64) -> EventSnapshot {
        EventSnapshot {
            kind: EventKind::AssistantText,
            display: display.to_string(),
            timestamp_ms,
        }
    }

    fn events_with_latest(display: &str, timestamp_ms: i64) -> WorkspaceEvents {
        let mut e = WorkspaceEvents::default();
        e.latest = Some(snap(display, timestamp_ms));
        e
    }

    #[test]
    fn select_row_returns_none_when_no_other_activity_or_attention() {
        let row = select_row(None, &[], 0);
        assert!(row.is_none());
    }

    #[test]
    fn select_row_attention_wins_over_recent_event() {
        let attention = events_with_latest("attention-evt", 5_000);
        let recent = events_with_latest("recent-evt", 9_000);
        let candidates_owned = vec![
            ws(1, "blocked", Some(attention), ActivityState::Waiting, true, None),
            ws(2, "busy", Some(recent), ActivityState::Idle, false, None),
        ];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 10_000).expect("row");
        assert_eq!(row.kind, UpdatesRowKind::Attention);
        assert_eq!(row.glyph, '⚠');
        assert!(row.text.contains("blocked"), "{}", row.text);
        assert!(row.text.contains("waiting"), "{}", row.text);
    }

    #[test]
    fn select_row_falls_back_to_most_recent_event() {
        let older = events_with_latest("older-evt", 1_000);
        let newer = events_with_latest("newer-evt", 8_000);
        let candidates_owned = vec![
            ws(1, "older", Some(older), ActivityState::Idle, false, None),
            ws(2, "newer", Some(newer), ActivityState::Active, false, None),
        ];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 10_000).expect("row");
        assert_eq!(row.kind, UpdatesRowKind::Activity);
        assert_eq!(row.glyph, '●');
        assert!(row.text.contains("newer:"), "{}", row.text);
        assert!(row.text.contains("newer-evt"), "{}", row.text);
    }

    #[test]
    fn select_row_excludes_currently_attached() {
        let evt = events_with_latest("evt", 5_000);
        let candidates_owned = vec![ws(1, "self", Some(evt), ActivityState::Idle, false, None)];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(Some(WorkspaceId(1)), &candidates, 10_000);
        assert!(row.is_none());
    }

    #[test]
    fn select_row_ignores_stale_events() {
        // event at t=0, now=120_000 ms → 120s old, > 60s threshold.
        let stale = events_with_latest("stale-evt", 0);
        let candidates_owned = vec![ws(1, "old", Some(stale), ActivityState::Idle, false, None)];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 120_000);
        assert!(row.is_none());
    }

    #[test]
    fn select_row_awaiting_tool_renders_tool_name() {
        let candidates_owned = vec![ws(
            1,
            "ws",
            None,
            ActivityState::Awaiting,
            true,
            Some(("Bash".to_string(), 8_000)),
        )];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 10_000).expect("row");
        assert!(
            row.text.contains("awaiting permission: Bash"),
            "{}",
            row.text
        );
        assert!(row.text.contains("(2s)"), "{}", row.text);
    }

    #[test]
    fn format_age_thresholds() {
        assert_eq!(format_age(0), "0s");
        assert_eq!(format_age(59_999), "59s");
        assert_eq!(format_age(60_000), "1m");
        assert_eq!(format_age(3_599_000), "59m");
        assert_eq!(format_age(3_600_000), "1h");
        assert_eq!(format_age(-500), "0s"); // negative delta clamps
    }
}
```

- [ ] **Step 3: Run tests, verify pass**

Run: `cargo test --lib ui::updates_bar -- --test-threads=1`
Expected: 7 passed, 0 failed.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src/ui/updates_bar.rs src/ui/mod.rs
git commit -m "feat(ui): updates_bar::select_row + age formatter"
```

---

## Task 2: `Modal::UpdatesPanel` variant + Esc dismissal

**Files:**
- Modify: `src/ui/modal.rs` (add variant)
- Modify: `src/app.rs` (`handle_key_modal` arm)

Adds the modal state machine for the panel — opening will come in Task 3.

- [ ] **Step 1: Write the failing test**

In `src/app.rs`, find the existing `#[cfg(test)] mod pm_state_tests` (added in Task 7 of the PM plan). Append:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_esc_closes() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel);
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_none(), "Esc should close UpdatesPanel");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_swallows_other_keys() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel);
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Char('q'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            app.modal.is_some(),
            "q should not dismiss UpdatesPanel"
        );
        assert!(!app.quit, "q should not propagate to App::quit");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib app::pm_state_tests::updates_panel -- --test-threads=1`
Expected: FAIL — `Modal::UpdatesPanel` variant does not exist.

- [ ] **Step 3: Add the variant**

In `src/ui/modal.rs`, find the `pub enum Modal` definition and add the new variant at the end:

```rust
#[derive(Debug, Clone)]
pub enum Modal {
    NewWorkspace {
        repo_id: RepoId,
        name_buffer: String,
    },
    ConfirmArchive {
        workspace_id: crate::store::WorkspaceId,
        name: String,
    },
    SetupRunning {
        log: Vec<String>,
    },
    Error {
        message: String,
    },
    UpdatesPanel,
}
```

- [ ] **Step 4: Handle the new variant in `modal::render`**

In `src/ui/modal.rs`, find the `pub fn render(f, area, modal, theme)` function. The body has `match modal { ... }` returning `(title, body)`. The new variant must not be matched here (because the panel needs live `App` data and is rendered via `render_updates_panel` in Task 4). Add an early return so the function is total:

Find the line:
```rust
pub fn render(f: &mut Frame, area: Rect, modal: &Modal, theme: &Theme) {
    let rect = centered(area, 60, 12);
```

Replace with:
```rust
pub fn render(f: &mut Frame, area: Rect, modal: &Modal, theme: &Theme) {
    // UpdatesPanel is rendered by `render_updates_panel` directly from
    // `draw()` because it needs live App state. This function should
    // never be called with UpdatesPanel; guard defensively.
    if matches!(modal, Modal::UpdatesPanel) {
        return;
    }
    let rect = centered(area, 60, 12);
```

- [ ] **Step 5: Handle the new variant in `handle_key_modal`**

In `src/app.rs`, find `async fn handle_key_modal`. It has a `match modal { ... }` matching the cloned `Modal`. Add a new arm BEFORE the trailing `_ => {}` (or the existing final arm, whatever it is):

```rust
        Modal::UpdatesPanel => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
```

- [ ] **Step 6: Run tests, verify pass**

Run: `cargo test --lib -- --test-threads=1`
Expected: All tests pass including the two new modal tests.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/ui/modal.rs src/app.rs
git commit -m "feat(modal): UpdatesPanel variant with Esc dismissal"
```

---

## Task 3: `Ctrl-a u` chord in attached + attached-pm

**Files:**
- Modify: `src/app.rs` (extend chord match in `handle_key_attached` and `handle_key_attached_pm`)

- [ ] **Step 1: Write the failing tests**

In `src/app.rs`'s `pm_state_tests` module, append:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_a_u_in_attached_opens_updates_panel() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Simulate a workspace existing in App state so handle_key_attached
        // doesn't bounce to Dashboard. We can't easily spawn a real session
        // for this unit test; instead use the attached-pm path which only
        // checks `app.pm`, then add a stub PM session via a non-running
        // sentinel. For attached (workspace) we'd need a Session — skip and
        // cover this via the attached-pm test instead.
        // Re-route: directly invoke the chord behavior on the pm handler.
        let _ = &mut app;
    }
```

(Note: this test is intentionally minimal — Task 3's full coverage comes
from the attached-pm test below since we don't easily have a workspace
session to attach to in unit tests. Delete the placeholder above before
committing.)

Replace the placeholder with the real attached-pm test:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_a_u_in_attached_pm_opens_updates_panel() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Manually spawn a PM session so handle_key_attached_pm has one.
        let cwd = PathBuf::from(".");
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
        };
        let s = app.sessions.spawn_pm(&cwd, 80, 24, mode).unwrap();
        app.pm = Some(s);
        app.view = crate::ui::View::AttachedPm;

        // Send Ctrl-a then 'u'.
        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.ctrl_a_pending);

        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.ctrl_a_pending);
        assert!(matches!(
            app.modal,
            Some(crate::ui::modal::Modal::UpdatesPanel)
        ));

        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::pm_state_tests::ctrl_a_u -- --test-threads=1`
Expected: FAIL — `u` arm in chord handler doesn't exist.

- [ ] **Step 3: Extend `handle_key_attached_pm`**

In `src/app.rs`, find `async fn handle_key_attached_pm`. The `if app.ctrl_a_pending { match k.code { ... } }` block has arms for `Char('d')` and `Char('a')`. Add a new arm BEFORE the trailing `_ => return Ok(())`:

```rust
            KeyCode::Char('u') => {
                app.modal = Some(crate::ui::modal::Modal::UpdatesPanel);
                return Ok(());
            }
```

- [ ] **Step 4: Extend `handle_key_attached`**

In `src/app.rs`, find `async fn handle_key_attached`. Same chord block. Add the same new arm:

```rust
            KeyCode::Char('u') => {
                app.modal = Some(crate::ui::modal::Modal::UpdatesPanel);
                return Ok(());
            }
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib -- --test-threads=1`
Expected: all pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): Ctrl-a u opens updates panel from attached views"
```

---

## Task 4: `modal::render_updates_panel`

**Files:**
- Modify: `src/ui/modal.rs` (add `render_updates_panel` function)

Renders the floating window with workspaces grouped by repo. Pure function over slices.

- [ ] **Step 1: Write the failing test**

In `src/app.rs`'s `pm_state_tests` module, append:

```rust
    #[test]
    fn updates_panel_render_shows_grouped_workspaces() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let store = Store::open_in_memory().unwrap();
        let repo1 = store
            .add_repo(std::path::Path::new("/tmp/r1"), "repo-alpha", "")
            .unwrap();
        let ws1 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo1,
                name: "alpha-ws",
                branch: "repo-alpha/alpha-ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha-ws"),
            })
            .unwrap();
        store
            .set_workspace_state(ws1, WorkspaceState::Ready)
            .unwrap();
        let repo2 = store
            .add_repo(std::path::Path::new("/tmp/r2"), "repo-beta", "")
            .unwrap();
        let ws2 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo2,
                name: "beta-ws",
                branch: "repo-beta/beta-ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/beta-ws"),
            })
            .unwrap();
        store
            .set_workspace_state(ws2, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel);

        let backend = TestBackend::new(100, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Workspace updates"),
            "missing panel title:\n{rendered}"
        );
        assert!(
            rendered.contains("repo-alpha"),
            "missing repo header:\n{rendered}"
        );
        assert!(
            rendered.contains("alpha-ws"),
            "missing workspace row:\n{rendered}"
        );
        assert!(
            rendered.contains("repo-beta"),
            "missing repo header:\n{rendered}"
        );
        assert!(
            rendered.contains("beta-ws"),
            "missing workspace row:\n{rendered}"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::pm_state_tests::updates_panel_render -- --test-threads=1`
Expected: FAIL — the panel isn't rendered yet (Task 5 wires it into `draw`, but the function doesn't exist either).

- [ ] **Step 3: Implement `render_updates_panel`**

In `src/ui/modal.rs`, append a new function:

```rust
use crate::events::WorkspaceEvents;
use crate::store::{Repo, RepoId, Workspace, WorkspaceId, WorkspaceState};
use std::collections::{HashMap, HashSet};

/// Render the floating workspace-updates panel. Reads live App state via
/// borrowed slices so the panel updates on every render tick.
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    repos: &[Repo],
    workspaces: &[(RepoId, Workspace)],
    events: &HashMap<WorkspaceId, WorkspaceEvents>,
    activity: &HashMap<WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<WorkspaceId>,
    awaiting: &HashMap<WorkspaceId, (String, i64)>,
    now_ms: i64,
    theme: &Theme,
) {
    // Sizing: ~80 cols wide, ~25 rows tall, but never larger than the area.
    let w = area.width.min(80).max(20);
    let h = area.height.min(25).max(8);
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Workspace updates ")
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    let mut lines: Vec<Line> = Vec::new();
    for repo in repos {
        lines.push(Line::from(Span::styled(
            repo.name.clone(),
            theme.header_style(),
        )));
        let mut ws_for_repo: Vec<&Workspace> = workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .collect();
        // Sort: attention first (by most recent), then active/idle by recent,
        // then resumable, then off, then failed.
        ws_for_repo.sort_by_key(|w| {
            let attention = if needs_attention.contains(&w.id) { 0 } else { 1 };
            let activity_rank = match activity.get(&w.id).copied() {
                Some(crate::ui::updates_bar::ActivityState::Awaiting)
                | Some(crate::ui::updates_bar::ActivityState::Waiting) => 0,
                Some(crate::ui::updates_bar::ActivityState::Active)
                | Some(crate::ui::updates_bar::ActivityState::Idle) => 1,
                Some(crate::ui::updates_bar::ActivityState::Off) => 2,
                None => 3,
            };
            let failed = if w.state == WorkspaceState::Failed { 1 } else { 0 };
            let recency = -events
                .get(&w.id)
                .and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms))
                .unwrap_or(0);
            (attention, failed, activity_rank, recency)
        });
        if ws_for_repo.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no workspaces)".to_string(),
                theme.dim_style(),
            )));
        } else {
            for w in ws_for_repo {
                lines.push(workspace_row(
                    w,
                    events.get(&w.id),
                    activity.get(&w.id).copied(),
                    needs_attention.contains(&w.id),
                    awaiting.get(&w.id),
                    now_ms,
                    theme,
                ));
            }
        }
        lines.push(Line::from(""));
    }
    if repos.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no repos)".to_string(),
            theme.dim_style(),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).style(theme.dim_style()),
        body_area,
    );
    f.render_widget(
        Paragraph::new("[esc] close").style(theme.dim_style()),
        footer_area,
    );
}

fn workspace_row<'a>(
    w: &'a Workspace,
    events: Option<&'a WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&'a (String, i64)>,
    now_ms: i64,
    _theme: &Theme,
) -> Line<'a> {
    use crate::ui::updates_bar::{format_age, ActivityState};
    let glyph = if w.state == WorkspaceState::Failed {
        '✕'
    } else if needs_attention {
        '⚠'
    } else {
        match activity {
            Some(ActivityState::Active) | Some(ActivityState::Idle) => '●',
            Some(ActivityState::Awaiting) | Some(ActivityState::Waiting) => '⚠',
            Some(ActivityState::Off) | None => {
                if events.and_then(|e| e.latest.as_ref()).is_some() {
                    '↻'
                } else {
                    '○'
                }
            }
        }
    };
    let (status_text, age_anchor_ms) = if let Some((tool, ts)) = awaiting {
        (format!("awaiting permission: {tool}"), Some(*ts))
    } else if needs_attention {
        ("waiting".to_string(), events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms)))
    } else if matches!(
        activity,
        Some(ActivityState::Active) | Some(ActivityState::Idle)
    ) {
        let text = events
            .and_then(|e| e.latest.as_ref().map(|s| s.display.clone()))
            .unwrap_or_else(|| "active".to_string());
        let ts = events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms));
        (text, ts)
    } else if w.state == WorkspaceState::Failed {
        ("failed".to_string(), None)
    } else if events.and_then(|e| e.latest.as_ref()).is_some() {
        ("resumable".to_string(), None)
    } else {
        ("no session".to_string(), None)
    };
    let age = age_anchor_ms.map(|t| format_age(now_ms.saturating_sub(t)));
    let suffix = match age {
        Some(a) => format!(" ({a})"),
        None => String::new(),
    };
    Line::from(format!(
        "  {glyph} {:<20} {}{}",
        w.name, status_text, suffix
    ))
}
```

- [ ] **Step 4: Run tests, verify pass after Task 5 wires it up**

The test in Step 1 will still fail until Task 5 wires the dispatch in `draw()`. Don't commit yet — proceed to Task 5 immediately.

Run: `cargo build --lib`
Expected: compiles cleanly.

- [ ] **Step 5: (Combined with Task 5 commit; do not commit separately)**

---

## Task 5: `draw()` dispatch for `UpdatesPanel`

**Files:**
- Modify: `src/app.rs` (modal dispatch in `draw()`)

This finishes the integration started in Task 4. Single commit covers both.

- [ ] **Step 1: Modify the modal dispatch in `draw()`**

In `src/app.rs`, find the existing block at the end of `draw()`:

```rust
    if let Some(m) = &app.modal {
        modal::render(f, area, m, &app.theme);
    }
```

Replace with:

```rust
    if let Some(m) = &app.modal {
        match m {
            crate::ui::modal::Modal::UpdatesPanel => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let mut awaiting: std::collections::HashMap<
                    crate::store::WorkspaceId,
                    (String, i64),
                > = std::collections::HashMap::new();
                for (_rid, w) in &app.workspaces {
                    if let Some(a) = app.awaiting_permission(w.id) {
                        awaiting.insert(w.id, a);
                    }
                }
                let activity_translated: std::collections::HashMap<
                    crate::store::WorkspaceId,
                    crate::ui::updates_bar::ActivityState,
                > = app
                    .workspace_activity
                    .iter()
                    .map(|(k, v)| (*k, translate_activity(*v)))
                    .collect();
                crate::ui::modal::render_updates_panel(
                    f,
                    area,
                    &app.repos,
                    &app.workspaces,
                    &app.workspace_events,
                    &activity_translated,
                    &app.workspace_needs_attention,
                    &awaiting,
                    now_ms,
                    &app.theme,
                );
            }
            other => modal::render(f, area, other, &app.theme),
        }
    }
```

- [ ] **Step 2: Add `translate_activity` helper**

`ActivityState` in `app.rs` and `ActivityState` in `ui::updates_bar` are separate types to keep `updates_bar` decoupled from `app.rs`. Add a private translation helper near the bottom of `src/app.rs`:

```rust
fn translate_activity(a: ActivityState) -> crate::ui::updates_bar::ActivityState {
    use crate::ui::updates_bar::ActivityState as U;
    match a {
        ActivityState::Active => U::Active,
        ActivityState::Idle => U::Idle,
        ActivityState::Waiting => U::Waiting,
        ActivityState::Awaiting => U::Awaiting,
        ActivityState::Off => U::Off,
    }
}
```

- [ ] **Step 3: Run tests, verify pass**

Run: `cargo test --lib -- --test-threads=1`
Expected: All tests pass including `updates_panel_render_shows_grouped_workspaces`.

Run: `cargo test --workspace -- --test-threads=1`
Expected: all pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit (covers Task 4 and Task 5 together)**

```bash
git add src/ui/modal.rs src/app.rs
git commit -m "feat(ui): render_updates_panel + draw() dispatch"
```

---

## Task 6: Status row in `attached::render` + footer hint

**Files:**
- Modify: `src/ui/attached.rs`

Adds the conditional status row above the existing footer and updates the footer hint text to mention `[Ctrl-a u] updates`.

- [ ] **Step 1: Write the failing tests**

In `src/app.rs`'s `pm_state_tests` module, append:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_view_shows_status_row_with_other_workspace_event() {
        use crate::events::{EventKind, EventSnapshot, WorkspaceEvents};
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let attached_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "attached-here",
                branch: "repo/attached-here",
                worktree_path: std::path::Path::new("/tmp/wsx-test/attached"),
            })
            .unwrap();
        store
            .set_workspace_state(attached_id, WorkspaceState::Ready)
            .unwrap();
        let other_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "the-other",
                branch: "repo/the-other",
                worktree_path: std::path::Path::new("/tmp/wsx-test/other"),
            })
            .unwrap();
        store
            .set_workspace_state(other_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Spawn a session for the attached workspace so the view has a PTY to render.
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
        };
        app.sessions
            .spawn(attached_id, std::path::Path::new("."), 80, 24, mode)
            .unwrap();
        app.view = crate::ui::View::Attached(attached_id);
        // Give "the-other" a recent event.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut ev = WorkspaceEvents::default();
        ev.latest = Some(EventSnapshot {
            kind: EventKind::AssistantText,
            display: "ran cargo test".to_string(),
            timestamp_ms: now_ms - 3000,
        });
        app.workspace_events.insert(other_id, ev);

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("the-other"),
            "expected status row mention of the other workspace:\n{rendered}"
        );
        assert!(
            rendered.contains("ran cargo test"),
            "expected status row event text:\n{rendered}"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_view_no_status_row_when_no_other_activity() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let attached_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "only-one",
                branch: "repo/only-one",
                worktree_path: std::path::Path::new("/tmp/wsx-test/only"),
            })
            .unwrap();
        store
            .set_workspace_state(attached_id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
        };
        app.sessions
            .spawn(attached_id, std::path::Path::new("."), 80, 24, mode)
            .unwrap();
        app.view = crate::ui::View::Attached(attached_id);

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        // The bottom row is the footer with "Ctrl-a d detach". The second-
        // to-last row should NOT contain a status indicator.
        let h = buf.area.height;
        let second_to_last: String = (0..buf.area.width)
            .map(|x| buf[(x, h - 2)].symbol())
            .collect();
        assert!(
            !second_to_last.contains('⚠'),
            "unexpected attention glyph in row {}: {second_to_last:?}",
            h - 2
        );
        assert!(
            !second_to_last.contains('●'),
            "unexpected activity glyph in row {}: {second_to_last:?}",
            h - 2
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib app::pm_state_tests::attached_view -- --test-threads=1`
Expected: first test FAILS (status row not rendered yet); second test may pass spuriously (no row exists yet) but that's OK — we want it to stay passing after Task 6.

- [ ] **Step 3: Update `attached::render`**

Replace the body of `pub fn render` in `src/ui/attached.rs`:

```rust
use crate::pty::render::render_screen;
use crate::pty::session::Session;
use crate::ui::theme::Theme;
use crate::ui::updates_bar::{self, UpdatesRow, UpdatesRowKind, WorkspaceUpdateInfo};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

/// Render the attached-workspace view. When `status_row` is `Some`, a
/// one-line indicator showing another workspace's update is inserted
/// above the footer; when `None`, the term gets that row back.
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: &Arc<Session>,
    label: &str,
    status_row: Option<&UpdatesRow>,
    theme: &Theme,
) {
    let status_height = if status_row.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(status_height),
            Constraint::Length(1),
        ])
        .split(area);
    let term_area = chunks[0];
    let status_area = chunks[1];
    let footer_area = chunks[2];

    let parser = session.parser.lock().unwrap();
    let screen = parser.screen();
    render_screen(screen, f.buffer_mut(), term_area);
    let (cy, cx) = screen.cursor_position();
    if !screen.hide_cursor() {
        f.set_cursor_position((term_area.x + cx, term_area.y + cy));
    }
    drop(parser);

    if let Some(row) = status_row {
        let style = match row.kind {
            UpdatesRowKind::Attention => theme.warn_style(),
            UpdatesRowKind::Activity => theme.ok_style(),
        };
        let text = format!(" {} {}", row.glyph, row.text);
        f.render_widget(Paragraph::new(text).style(style), status_area);
    }

    let footer = format!(
        " {label}   [Ctrl-a d] detach   [Ctrl-a u] updates   [Ctrl-a a] send Ctrl-a "
    );
    f.render_widget(Paragraph::new(footer).style(theme.dim_style()), footer_area);
}

pub fn resize_session(session: &Arc<Session>, area: Rect) {
    let _ = session.resize(area.width, area.height.saturating_sub(1));
}
```

- [ ] **Step 4: Update the two callers of `attached::render`**

In `src/app.rs`, find both call sites:

```rust
attached::render(f, area, &session, &label, &app.theme);
```

(One in the `View::Attached(id)` arm, one in the `View::AttachedPm` arm.)

Replace each with a version that computes the status row first. For `View::Attached(id)`:

```rust
            if let Some(session) = app.sessions.get(*id) {
                let label = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == *id)
                    .map(|(_, w)| w.name.clone())
                    .unwrap_or_default();
                attached::resize_session(&session, area);
                let row = if matches!(
                    app.modal,
                    Some(crate::ui::modal::Modal::UpdatesPanel)
                ) {
                    None
                } else {
                    compute_status_row(app, Some(*id))
                };
                attached::render(f, area, &session, &label, row.as_ref(), &app.theme);
            }
```

For `View::AttachedPm`:

```rust
        View::AttachedPm => {
            if let Some(session) = app.pm.as_ref() {
                attached::resize_session(session, area);
                let row = if matches!(
                    app.modal,
                    Some(crate::ui::modal::Modal::UpdatesPanel)
                ) {
                    None
                } else {
                    compute_status_row(app, None)
                };
                attached::render(f, area, session, "project-manager", row.as_ref(), &app.theme);
            } else {
                app.view = View::Dashboard;
            }
        }
```

Then add the `compute_status_row` helper near the bottom of `src/app.rs` (next to `translate_activity`):

```rust
fn compute_status_row(
    app: &App,
    attached_id: Option<crate::store::WorkspaceId>,
) -> Option<crate::ui::updates_bar::UpdatesRow> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let candidates: Vec<crate::ui::updates_bar::WorkspaceUpdateInfo> = app
        .workspaces
        .iter()
        .map(|(_, w)| {
            let activity = app
                .workspace_activity
                .get(&w.id)
                .copied()
                .map(translate_activity)
                .unwrap_or(crate::ui::updates_bar::ActivityState::Off);
            crate::ui::updates_bar::WorkspaceUpdateInfo {
                id: w.id,
                name: w.name.as_str(),
                events: app.workspace_events.get(&w.id),
                activity,
                needs_attention: app.workspace_needs_attention.contains(&w.id),
                awaiting_tool: app.awaiting_permission(w.id),
            }
        })
        .collect();
    crate::ui::updates_bar::select_row(attached_id, &candidates, now_ms)
}
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib -- --test-threads=1`
Expected: all pass including the two new attached-view tests.

Run: `cargo test --workspace -- --test-threads=1`
Expected: all pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/ui/attached.rs src/app.rs
git commit -m "feat(ui): status row above attached footer + Ctrl-a u hint"
```

---

## Task 7: README + final commit closing #11

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add the Ctrl-a u row to the Attached keybindings table**

In `README.md`, find the table under `### Attached workspace`. Add a new row after the `Ctrl-a a` row:

```markdown
| `Ctrl-a u` | Open the floating updates panel (shows other workspaces' state) |
```

- [ ] **Step 2: Add the "Workspace updates panel" section**

In `README.md`, find the existing `## Dashboard status indicators` section. After it (before the next `##` heading), insert:

```markdown
## Workspace updates panel

When you're attached to a workspace (full-screen claude session) or the
project manager pane is expanded full-screen, wsx still tracks the other
workspaces in the background. Two affordances surface that:

- A single-row status indicator above the footer, shown only when another
  workspace needs attention or has produced output in the last 60 seconds.
  Format: `⚠ <name> awaiting permission: <tool> (<age>)` for attention,
  `● <name>: <event> (<age>)` for activity. The row collapses to nothing
  when there's nothing to surface, giving claude the row back.

- A floating panel via `Ctrl-a u` listing ALL workspaces grouped by repo,
  with their current state and latest event. Press `Esc` to close. The
  panel re-renders live, so ages count up and attention flags appear/clear
  in real time.
```

- [ ] **Step 3: Final build + test sweep**

Run: `cargo test --workspace -- --test-threads=1`
Expected: all pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

Run: `cargo build --release`
Expected: succeeds.

- [ ] **Step 4: Final commit with `Closes #11`**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
feat(ui): workspace updates panel (#11)

Surfaces other-workspace activity while attached:
- Status row above the attached-view footer, shown when another
  workspace needs attention or has output in the last 60s.
- Floating Ctrl-a u panel listing all workspaces grouped by repo,
  with attention-needing rows first; Esc to close. Live re-rendering.

Modal::UpdatesPanel uses the existing hard-modal pattern but renders
via a dedicated render_updates_panel that takes borrowed App slices.

Closes #11
EOF
)"
```

---

## Self-Review

**1. Spec coverage:**

| Spec section | Covered by |
|---|---|
| `updates_bar` pure module | Task 1 |
| `Modal::UpdatesPanel` variant | Task 2 |
| `handle_key_modal` arm | Task 2 |
| `Ctrl-a u` chord in attached + attached-pm | Task 3 |
| `render_updates_panel` function | Task 4 |
| `draw()` dispatch + slice plumbing | Task 5 |
| Status row layout (3-chunk) in attached.rs | Task 6 |
| Footer hint update | Task 6 |
| Suppress status row when modal is open | Task 6 (`if matches!(app.modal, …)` guard) |
| README updates | Task 7 |

No gaps.

**2. Placeholder scan:** No "TBD", "TODO", "etc.", "handle edge cases", or "similar to Task N" in any task.

**3. Type consistency:**
- `UpdatesRow { glyph, kind, text }`, `UpdatesRowKind::{Attention, Activity}`, `WorkspaceUpdateInfo<'a>` — consistent across Tasks 1, 6.
- `ActivityState` — two enums by the same name exist (one in `app.rs`, one in `ui::updates_bar`). Task 5 introduces `translate_activity` to convert; all subsequent uses route through it. Consistent.
- `render_updates_panel` signature — consistent between Task 4 (declaration) and Task 5 (call site).
- `attached::render` signature gains a `status_row: Option<&UpdatesRow>` param — Task 6 declares it; both call sites updated in the same task.
