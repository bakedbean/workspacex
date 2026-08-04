# Workspace Rename via Actions Modal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user rename a workspace from the TUI via the `?` workspace-actions card (`r` opens an in-modal rename input).

**Architecture:** A new `Modal::RenameWorkspace` variant follows the existing hand-rolled text-input pattern (`Modal::NewWorkspace` buffer + `Modal::ProcessList`-style inline `notice`). `r` is handled only inside the `Modal::WorkspaceActions` key arm (NOT forwarded to the dashboard — bare `r` there is taken by the PM-refresh nudge). Enter normalizes the buffer with a new `normalize_slug` helper and calls the existing core `crate::data::workspace::rename` inline, then `app.refresh()`.

**Tech Stack:** Rust, ratatui, crossterm, tokio, sqlite (rusqlite via `Store`). Repo root: this worktree. Spec: `docs/superpowers/specs/2026-08-04-workspace-rename-modal-design.md`.

## Global Constraints

- Never commit to `main` — work happens on this branch (`eg/rename-workspace-modal`).
- CI gates are separate: run `cargo fmt --check`, `cargo clippy --all-targets`, and `cargo test` before each commit (clippy passing ≠ fmt clean).
- Known flaky test (unrelated): `click_chip_auto_spawns_session_when_missing` — a failure there alone is not caused by this work.
- Worktree dir and live tmux `session_ref` are deliberately NOT touched by rename (established invariant, see spec).
- The `Modal` enum is `Clone`; modal key handlers destructure a cloned modal by value and reassign `app.modal` on every state change (see `Modal::NewWorkspace` arm, `src/app/input.rs:1233`). Follow that pattern.

---

### Task 1: `normalize_slug` helper

**Files:**
- Modify: `src/data/workspace.rs` (helper next to `slugify_prompt` at `:497`; tests in the existing `#[cfg(test)] mod tests` at `:540`)

**Interfaces:**
- Produces: `pub fn normalize_slug(text: &str) -> Option<String>` in `crate::data::workspace` — kebab-case normalization of user-typed text; `None` only when nothing alphanumeric remains. Unlike `slugify_prompt`, it never drops stopwords and has no minimum length (a deliberately typed `wip-ci` or `x` passes through).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/data/workspace.rs` (next to `slugify_basic` at `:792`):

```rust
    #[test]
    fn normalize_slug_cases() {
        // Exact slugs pass through untouched.
        assert_eq!(normalize_slug("wip-ci"), Some("wip-ci".into()));
        // Lowercasing + punctuation → dashes.
        assert_eq!(normalize_slug("Fix Login!!"), Some("fix-login".into()));
        // Dash runs collapse, edges trim.
        assert_eq!(normalize_slug("--a--b--"), Some("a-b".into()));
        // No stopword dropping, no length floor (contrast slugify_prompt).
        assert_eq!(normalize_slug("the"), Some("the".into()));
        assert_eq!(normalize_slug("x"), Some("x".into()));
        // Nothing alphanumeric → None.
        assert_eq!(normalize_slug("..."), None);
        assert_eq!(normalize_slug(""), None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test normalize_slug_cases`
Expected: compile error — `normalize_slug` not found.

- [ ] **Step 3: Write minimal implementation**

Add directly below `slugify_prompt` (after `src/data/workspace.rs:523`):

```rust
/// Normalize user-typed text into a kebab-case slug: lowercase, map
/// non-alphanumerics to '-', collapse dash runs, trim edge dashes.
/// Unlike `slugify_prompt` this never drops words and has no minimum
/// length — the user typed exactly the slug they want. Returns `None`
/// only when nothing alphanumeric remains.
pub fn normalize_slug(text: &str) -> Option<String> {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let words: Vec<&str> = cleaned.split('-').filter(|s| !s.is_empty()).collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join("-"))
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test normalize_slug_cases`
Expected: PASS

- [ ] **Step 5: Gate + commit**

```bash
cargo fmt --check && cargo clippy --all-targets && cargo test --lib data::workspace
git add src/data/workspace.rs
git commit -m "feat: add normalize_slug for user-typed workspace names"
```

---

### Task 2: `Modal::RenameWorkspace` variant + rendering

**Files:**
- Modify: `src/ui/modal/mod.rs` (`Modal` enum at `:45`; generic `render()` match at `:219`; `WorkspaceActions` body at `:344`)
- Test: `src/app/input_tests.rs` (render tests via `TestBackend` + `draw_for_test`, pattern at `:28-40`)

**Interfaces:**
- Produces: enum variant used verbatim by Task 3:

```rust
    RenameWorkspace {
        workspace_id: crate::data::store::WorkspaceId,
        /// Pre-filled with the current name; edited in place.
        name_buffer: String,
        /// Inline error line (e.g. rename failure); cleared on next edit.
        notice: Option<String>,
    },
```

- [ ] **Step 1: Write the failing render tests**

Add a new `mod rename_modal_tests` block in `src/app/input_tests.rs` (sibling of `pm_state_tests`; copy its `use super::*;` + imports style):

```rust
#[cfg(test)]
mod rename_modal_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn screen_text(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn app_with_workspace() -> (App, crate::data::store::WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        (app, ws_id)
    }

    #[test]
    fn workspace_actions_card_lists_rename() {
        let (mut app, ws_id) = app_with_workspace();
        app.dashboard.selection =
            Some(crate::app::SelectionTarget::Workspace(ws_id));
        app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        assert!(
            screen_text(&term).contains("rename"),
            "actions card must list the rename action"
        );
    }

    #[test]
    fn rename_modal_renders_buffer_and_notice() {
        let (mut app, ws_id) = app_with_workspace();
        app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
            workspace_id: ws_id,
            name_buffer: "alpha-two".to_string(),
            notice: Some("rename failed: boom".to_string()),
        });
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let text = screen_text(&term);
        assert!(text.contains("alpha-two"), "buffer must render; got {text:?}");
        assert!(
            text.contains("rename failed: boom"),
            "notice must render; got {text:?}"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test rename_modal_tests`
Expected: compile error — no `RenameWorkspace` variant.

- [ ] **Step 3: Add the variant and rendering**

In `src/ui/modal/mod.rs`:

(a) Add the variant from the Interfaces block above to `enum Modal` (after `ProcessList` at `:102`).

(b) In the generic `render()` match (`:219`), add an arm (place after the `Modal::WorkspaceActions` arm). This modal is self-contained (no live App state), so it goes through generic `render()` — do NOT add it to the guard list at `:206`:

```rust
        Modal::RenameWorkspace {
            name_buffer, notice, ..
        } => {
            let notice_line = notice
                .as_deref()
                .map(|n| format!("{n}\n"))
                .unwrap_or_default();
            (
                "rename workspace",
                format!(
                    "name: {name_buffer}\u{2588}\n\n{notice_line}[enter] rename   [esc] cancel"
                ),
            )
        }
```

(c) Extend the `Modal::WorkspaceActions` body (`:344`) — the `c` line gains a second column:

```rust
        Modal::WorkspaceActions => (
            "workspace actions",
            "These apply to the selected workspace:\n\n  \
             e   edit        t   term\n  \
             v   diff        g   lazygit\n  \
             c   chronox     r   rename\n\n  \
             ?/Esc  close"
                .to_string(),
        ),
```

Note: adding a variant will trip exhaustive matches elsewhere — the compiler will point at them (expected: the key handler in `src/app/input.rs`; add a temporary `Modal::RenameWorkspace { .. } => {}` arm there, replaced in Task 3).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test rename_modal_tests`
Expected: both PASS (`workspace_actions_card_lists_rename` and `rename_modal_renders_buffer_and_notice`).

- [ ] **Step 5: Gate + commit**

```bash
cargo fmt --check && cargo clippy --all-targets && cargo test rename_modal_tests
git add src/ui/modal/mod.rs src/app/input.rs src/app/input_tests.rs
git commit -m "feat: RenameWorkspace modal variant, render arm, actions-card row"
```

---

### Task 3: Key handling — open, edit, apply

**Files:**
- Modify: `src/app/input.rs` (`Modal::WorkspaceActions` arm at `:1411`; replace the Task-2 stub arm for `RenameWorkspace`)
- Test: `src/app/input_tests.rs` (extend `mod rename_modal_tests` from Task 2)

**Interfaces:**
- Consumes: `crate::data::workspace::normalize_slug(&str) -> Option<String>` (Task 1); `Modal::RenameWorkspace { workspace_id, name_buffer, notice }` (Task 2); existing `crate::data::workspace::rename(&Store, &Repo, &Workspace, &str) -> Result<()>` (`src/data/workspace.rs:527`); `App::refresh()` (`src/app.rs:733`); `app.selected_target()` / `SelectionTarget::Workspace` (`src/app.rs:843`).
- Produces: nothing consumed later — this is the final wiring task.

- [ ] **Step 1: Write the failing behavior tests**

Append inside `mod rename_modal_tests` in `src/app/input_tests.rs`. The `shared` argument follows the existing dummy-shared pattern (`repo_settings_modal_j_k_aliases_down_up`, `src/app/input_tests.rs:560`):

```rust
    fn dummy_shared() -> std::sync::Arc<tokio::sync::Mutex<App>> {
        std::sync::Arc::new(tokio::sync::Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actions_card_r_opens_rename_prefilled() {
        let (mut app, ws_id) = app_with_workspace();
        app.dashboard.selection =
            Some(crate::app::SelectionTarget::Workspace(ws_id));
        app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
        let shared = dummy_shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.modal {
            Some(crate::ui::modal::Modal::RenameWorkspace {
                workspace_id,
                name_buffer,
                notice,
            }) => {
                assert_eq!(*workspace_id, ws_id);
                assert_eq!(name_buffer, "alpha", "buffer pre-fills current name");
                assert!(notice.is_none());
            }
            other => panic!("expected RenameWorkspace modal, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rename_modal_esc_cancels_without_changes() {
        let (mut app, ws_id) = app_with_workspace();
        app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
            workspace_id: ws_id,
            name_buffer: "alpha-two".to_string(),
            notice: None,
        });
        let shared = dummy_shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_none());
        let (_, ws) = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == ws_id)
            .unwrap();
        assert_eq!(ws.name, "alpha", "esc must not rename");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rename_modal_empty_buffer_shows_notice() {
        let (mut app, ws_id) = app_with_workspace();
        app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
            workspace_id: ws_id,
            name_buffer: "...".to_string(), // normalizes to None
            notice: None,
        });
        let shared = dummy_shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.modal {
            Some(crate::ui::modal::Modal::RenameWorkspace { notice, .. }) => {
                assert_eq!(notice.as_deref(), Some("name cannot be empty"));
            }
            other => panic!("modal must stay open with notice, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rename_modal_enter_renames_workspace_and_branch() {
        // Real git repo: `rename` runs `git branch -m`.
        let repo_dir = tempfile::TempDir::new().unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(repo_dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        r(&["init", "-q", "-b", "main"]);
        r(&["config", "user.email", "t@e"]);
        r(&["config", "user.name", "t"]);
        r(&["commit", "--allow-empty", "-q", "-m", "init"]);

        let store = Store::open_in_memory().unwrap();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let base = tempfile::TempDir::new().unwrap();
        let created = crate::data::workspace::create(
            &store,
            &repo,
            Some("alpha"),
            base.path(),
            false,
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let ws_id = created.workspace.id;

        let mut app = App::new(store, base.path().to_path_buf()).unwrap();
        app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
            workspace_id: ws_id,
            name_buffer: "Fix Bug!".to_string(), // exercises normalization too
            notice: None,
        });
        let shared = dummy_shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();

        assert!(app.modal.is_none(), "modal closes on success");
        let ws = app
            .store
            .workspaces(repo.id)
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws_id)
            .unwrap();
        assert_eq!(ws.name, "fix-bug");
        assert_eq!(ws.branch, "wsx/fix-bug");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rename_modal_git_failure_keeps_modal_with_notice() {
        // Repo path is not a git repo → `git branch -m` fails.
        let (mut app, ws_id) = app_with_workspace();
        app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
            workspace_id: ws_id,
            name_buffer: "beta".to_string(),
            notice: None,
        });
        let shared = dummy_shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.modal {
            Some(crate::ui::modal::Modal::RenameWorkspace { notice, .. }) => {
                assert!(
                    notice.as_deref().unwrap_or("").starts_with("rename failed"),
                    "got notice {notice:?}"
                );
            }
            other => panic!("modal must stay open on git failure, got {other:?}"),
        }
        let (_, ws) = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == ws_id)
            .unwrap();
        assert_eq!(ws.name, "alpha", "failed rename must not change the name");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rename_modal_typing_and_backspace_edit_buffer() {
        let (mut app, ws_id) = app_with_workspace();
        app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
            workspace_id: ws_id,
            name_buffer: "alpha".to_string(),
            notice: Some("stale".to_string()),
        });
        let shared = dummy_shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.modal {
            Some(crate::ui::modal::Modal::RenameWorkspace {
                name_buffer,
                notice,
                ..
            }) => {
                assert_eq!(name_buffer, "alphx");
                assert!(notice.is_none(), "editing clears a stale notice");
            }
            other => panic!("expected RenameWorkspace modal, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test rename_modal_tests`
Expected: the new tests FAIL (the stub arm from Task 2 ignores all keys; `r` in the actions card is inert).

- [ ] **Step 3: Implement the key handling**

In `src/app/input.rs`:

(a) In the `Modal::WorkspaceActions` arm (`:1411`), add a `Char('r')` case BEFORE the forwarded-keys case (`r` is handled here, never forwarded — bare dashboard `r` belongs to the PM-refresh nudge):

```rust
            // Rename is handled in-modal (not forwarded): bare `r` on the
            // dashboard is the PM-digest refresh nudge.
            KeyCode::Char('r') => {
                let ws = match app.selected_target() {
                    Some(SelectionTarget::Workspace(ws_id)) => app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == ws_id)
                        .map(|(_, w)| (w.id, w.name.clone())),
                    _ => None,
                };
                app.modal = ws.map(|(workspace_id, name_buffer)| {
                    Modal::RenameWorkspace {
                        workspace_id,
                        name_buffer,
                        notice: None,
                    }
                });
            }
```

(b) Replace the Task-2 stub arm with the full handler (same consumed-and-reassigned pattern as `Modal::NewWorkspace`, `:1233`):

```rust
        Modal::RenameWorkspace {
            workspace_id,
            mut name_buffer,
            notice: _,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Enter => {
                match crate::data::workspace::normalize_slug(&name_buffer) {
                    None => {
                        app.modal = Some(Modal::RenameWorkspace {
                            workspace_id,
                            name_buffer,
                            notice: Some("name cannot be empty".to_string()),
                        });
                    }
                    Some(slug) => {
                        let ws = app
                            .workspaces
                            .iter()
                            .find(|(_, w)| w.id == workspace_id)
                            .map(|(_, w)| w.clone());
                        let repo = ws.as_ref().and_then(|w| {
                            app.repos.iter().find(|r| r.id == w.repo_id).cloned()
                        });
                        match (ws, repo) {
                            (Some(ws), Some(repo)) if slug != ws.name => {
                                match crate::data::workspace::rename(
                                    &app.store, &repo, &ws, &slug,
                                )
                                .await
                                {
                                    Ok(()) => {
                                        app.modal = None;
                                        app.refresh()?;
                                    }
                                    Err(e) => {
                                        app.modal = Some(Modal::RenameWorkspace {
                                            workspace_id,
                                            name_buffer,
                                            notice: Some(format!("rename failed: {e}")),
                                        });
                                    }
                                }
                            }
                            // Unchanged name: nothing to do.
                            (Some(_), Some(_)) => {
                                app.modal = None;
                            }
                            // Workspace/repo vanished underneath (archived
                            // elsewhere): close quietly and resync.
                            _ => {
                                app.modal = None;
                                app.refresh()?;
                            }
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                name_buffer.pop();
                app.modal = Some(Modal::RenameWorkspace {
                    workspace_id,
                    name_buffer,
                    notice: None,
                });
            }
            KeyCode::Char(c)
                if !k.modifiers.contains(KeyModifiers::CONTROL)
                    && !k.modifiers.contains(KeyModifiers::ALT) =>
            {
                name_buffer.push(c);
                app.modal = Some(Modal::RenameWorkspace {
                    workspace_id,
                    name_buffer,
                    notice: None,
                });
            }
            _ => {}
        },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test rename_modal_tests`
Expected: all 8 tests in the module PASS.

- [ ] **Step 5: Full gate + commit**

```bash
cargo fmt --check && cargo clippy --all-targets && cargo test
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat: rename workspace from the actions modal"
```

(If only `click_chip_auto_spawns_session_when_missing` fails, rerun it once — known flaky PTY-timing test.)
