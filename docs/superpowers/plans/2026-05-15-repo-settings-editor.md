# Repo settings editor in TUI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Press `s` on a dashboard row to view + edit per-repo settings (`branch_prefix`, `custom_instructions`, `setup_script`, `archive_script`) via `$EDITOR`. wsx suspends raw mode, runs the editor, resumes, saves.

**Architecture:** New `Modal::RepoSettings` variant + renderer. New `pending_edit: Option<PendingEdit>` field on `App` decouples the modal from the terminal handle — the modal handler signals intent; the run loop performs the suspend/resume around `external::edit_in_editor`. Direct to main.

**Tech Stack:** Rust, crossterm (raw-mode toggle, alternate-screen), tokio (no new async), `$EDITOR` invocation via `std::process::Command::status` (blocking — fine while TUI is suspended). No new crate dependencies; tempfile suffix is hand-rolled from `process::id()` + `SystemTime::now()` nanos.

**Spec:** `docs/superpowers/specs/2026-05-15-repo-settings-editor-design.md`

---

## File Structure

- `src/external.rs` — new `edit_in_editor(initial, ext_hint) -> Result<Option<String>>` function. Plus tests using `EDITOR=/bin/true` and `EDITOR=/bin/false`.
- `src/app.rs` — new `RepoSettingField` enum, `PendingEdit` struct, `App.pending_edit` field. New `do_pending_edit` helper called from `run`. New `apply_repo_setting` dispatcher. New `s` arm in `handle_key_dashboard`. New `Modal::RepoSettings` arm in `handle_key_modal`. Run loop hook before `terminal.draw`.
- `src/ui/modal.rs` — new `Modal::RepoSettings { repo_id, selected }` variant. Early-return guard in `render` extended. New `render_repo_settings` function.
- `src/ui/dashboard.rs` — footer adds `[s] settings`.
- `README.md` — new "Per-repo settings (TUI)" subsection; dashboard keybinds table gains an `s` row.

No new files; no new crate deps.

---

### Task 1: `external::edit_in_editor` helper

**Files:**
- Modify: `src/external.rs`

A blocking helper that writes a tempfile, spawns `$EDITOR` on it, and returns the new content (or `None` if the editor exited non-zero).

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/external.rs`:

```rust
#[test]
fn edit_in_editor_returns_unchanged_when_editor_doesnt_write() {
    // Save / restore EDITOR around the test.
    let saved = std::env::var_os("EDITOR");
    unsafe { std::env::set_var("EDITOR", "/bin/true"); }
    let result = edit_in_editor("hello world", "txt");
    unsafe {
        match saved {
            Some(v) => std::env::set_var("EDITOR", v),
            None => std::env::remove_var("EDITOR"),
        }
    }
    assert_eq!(result.unwrap().as_deref(), Some("hello world"));
}

#[test]
fn edit_in_editor_returns_none_when_editor_exits_nonzero() {
    let saved = std::env::var_os("EDITOR");
    unsafe { std::env::set_var("EDITOR", "/bin/false"); }
    let result = edit_in_editor("anything", "txt");
    unsafe {
        match saved {
            Some(v) => std::env::set_var("EDITOR", v),
            None => std::env::remove_var("EDITOR"),
        }
    }
    assert!(result.unwrap().is_none());
}
```

- [ ] **Step 2: Run to confirm compile failure**

```
cargo test --lib external::tests::edit_in_editor 2>&1 | tail -10
```
Expected: undefined `edit_in_editor`.

- [ ] **Step 3: Implement `edit_in_editor`**

Add to `src/external.rs` (alongside the existing public openers):

```rust
/// Open `$EDITOR` (or `vi` if unset) on a tempfile prepopulated with
/// `initial`. Returns `Ok(Some(contents))` on a clean exit (the contents
/// may equal `initial` if the user didn't modify them), `Ok(None)` if
/// the editor exited non-zero (treat as cancel), or `Err` on tempfile/
/// spawn I/O failure.
///
/// `ext_hint` is the file extension (no leading dot) — `"sh"`, `"md"`,
/// `"txt"` — used so the editor picks the right syntax mode.
pub fn edit_in_editor(initial: &str, ext_hint: &str) -> Result<Option<String>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("wsx-edit-{pid}-{nanos}.{ext_hint}"));
    std::fs::write(&path, initial)?;

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| Error::Io(std::io::Error::other(format!("spawn {editor}: {e}"))))?;

    if !status.success() {
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }
    let new = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(Some(new))
}
```

- [ ] **Step 4: Run tests; expect pass**

```
cargo test --lib external::tests::edit_in_editor 2>&1 | tail -10
```
Expected: 2 tests pass.

- [ ] **Step 5: fmt + clippy**

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
Both must pass.

- [ ] **Step 6: Commit**

```
git add src/external.rs
git commit -m "feat(external): edit_in_editor opens \$EDITOR on a tempfile

Helper that prepopulates a tempfile, runs the configured editor,
returns the saved content (or None if the editor exited non-zero).
Used by the upcoming TUI repo-settings editor."
```

---

### Task 2: `Modal::RepoSettings` variant + renderer

**Files:**
- Modify: `src/ui/modal.rs`
- Modify: `src/app.rs` (extend `draw()` dispatch)

- [ ] **Step 1: Add types to `src/app.rs` first**

We need `RepoSettingField` visible to both `modal.rs` (for the renderer signature) and `app.rs`. Put it in `app.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSettingField {
    BranchPrefix,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
}

impl RepoSettingField {
    pub const ALL: [Self; 4] = [
        Self::BranchPrefix,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::BranchPrefix => "branch_prefix",
            Self::CustomInstructions => "custom_instructions",
            Self::SetupScript => "setup_script",
            Self::ArchiveScript => "archive_script",
        }
    }
}
```

Place these near the existing `pub enum SelectionTarget` declaration so they're top-level in `app.rs`.

- [ ] **Step 2: Add the modal variant**

In `src/ui/modal.rs`, extend the `Modal` enum:

```rust
RepoSettings {
    repo_id: crate::store::RepoId,
    selected: usize,
},
```

- [ ] **Step 3: Extend the early-return guard in `render()`**

```rust
if matches!(
    modal,
    Modal::UpdatesPanel { .. }
        | Modal::ProcessList { .. }
        | Modal::RepoSettings { .. }
) {
    return;
}
```

Also add the `Modal::RepoSettings { .. } => unreachable!("RepoSettings must not reach render()"),` arm for exhaustiveness inside the `match modal { ... }` block.

- [ ] **Step 4: Add `render_repo_settings`**

Below `render_process_list`, add:

```rust
/// Render the floating repo-settings modal. Live state — reads
/// current values from the borrowed `Repo` struct.
pub fn render_repo_settings(
    f: &mut Frame,
    area: Rect,
    repo_name: &str,
    repo: &crate::store::Repo,
    selected: usize,
    theme: &Theme,
) {
    let w = area.width.clamp(40, 90);
    let h = area.height.clamp(8, 16);
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);

    let title = format!(" Repo settings — {repo_name} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    let rows: [(crate::app::RepoSettingField, Option<&str>); 4] = [
        (
            crate::app::RepoSettingField::BranchPrefix,
            if repo.branch_prefix.is_empty() {
                None
            } else {
                Some(repo.branch_prefix.as_str())
            },
        ),
        (
            crate::app::RepoSettingField::CustomInstructions,
            repo.custom_instructions.as_deref(),
        ),
        (
            crate::app::RepoSettingField::SetupScript,
            repo.setup_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::ArchiveScript,
            repo.archive_script.as_deref(),
        ),
    ];

    let mut lines: Vec<Line> = Vec::new();
    for (i, (field, value)) in rows.iter().enumerate() {
        let label_pad = 22; // width of the longest label + breathing room
        let preview = value
            .map(|v| preview_value(v, 60))
            .unwrap_or_else(|| "(unset)".to_string());
        let body = format!("  {:<width$} {}", field.label(), preview, width = label_pad);
        let style = if value.is_none() {
            theme.dim_style()
        } else {
            Style::default()
        };
        if i == selected {
            lines.push(Line::from(Span::styled(body, theme.selected_style())));
        } else {
            lines.push(Line::from(Span::styled(body, style)));
        }
    }
    f.render_widget(Paragraph::new(lines), body_area);

    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [enter] edit   [d] clear   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
}

/// First non-empty line, trimmed and truncated. Used by render_repo_settings.
fn preview_value(s: &str, max: usize) -> String {
    let first_line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let trimmed = first_line.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn preview_value_returns_first_nonempty_line() {
        assert_eq!(preview_value("\n  \nhello\nworld", 60), "hello");
    }

    #[test]
    fn preview_value_truncates_with_ellipsis() {
        let long = "x".repeat(100);
        let out = preview_value(&long, 60);
        assert!(out.ends_with('\u{2026}'));
        assert_eq!(out.chars().count(), 60);
    }

    #[test]
    fn preview_value_empty_returns_empty() {
        assert_eq!(preview_value("", 60), "");
    }
}
```

- [ ] **Step 5: Extend `draw()` dispatch in `app.rs`**

Find the existing dispatch for `Modal::ProcessList` in `src/app.rs::draw`. Add a sibling arm for `RepoSettings`:

```rust
Some(crate::ui::modal::Modal::RepoSettings {
    repo_id,
    selected,
}) => {
    if let Some(repo) = app.repos.iter().find(|r| r.id == *repo_id) {
        let repo_name = repo.name.clone();
        crate::ui::modal::render_repo_settings(
            f,
            area,
            &repo_name,
            repo,
            *selected,
            &app.theme,
        );
    }
}
```

Place it consistently with the `ProcessList` arm — same `match` shape.

- [ ] **Step 6: Add a placeholder modal handler arm**

The `match modal { ... }` in `handle_key_modal` needs exhaustiveness for the new variant. Add a placeholder for now (Task 4 fills it out):

```rust
Modal::RepoSettings { .. } => {
    if k.code == KeyCode::Esc {
        app.modal = None;
    }
}
```

- [ ] **Step 7: Build + fmt + clippy + tests**

```
cargo fmt
cargo build --message-format=short 2>&1 | tail -8
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```
All must be clean. The `preview_value` tests (3) should pass.

- [ ] **Step 8: Commit**

```
git add src/app.rs src/ui/modal.rs
git commit -m "feat(ui): Modal::RepoSettings variant + renderer

Adds the RepoSettingField enum (in app.rs), the modal variant,
and the live-state renderer that shows current values for the
four per-repo settings. Modal isn't reachable from keybinds yet
— Task 4 wires it up."
```

---

### Task 3: `App.pending_edit` field + `PendingEdit` struct

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add `PendingEdit`**

Below the `RepoSettingField` enum from Task 2, add:

```rust
#[derive(Debug, Clone)]
pub struct PendingEdit {
    pub repo_id: crate::store::RepoId,
    pub field: RepoSettingField,
}
```

- [ ] **Step 2: Add the App field**

In the `App` struct, add (near `pub last_proc_scan_ms`):

```rust
/// Set by the repo-settings modal when the user presses Enter on a
/// field. The run loop detects this BEFORE the next draw, suspends
/// the TUI, invokes `external::edit_in_editor`, resumes, and saves.
pub pending_edit: Option<PendingEdit>,
```

And initialize in `App::new`:

```rust
pending_edit: None,
```

- [ ] **Step 3: Build to confirm clean**

```
cargo build --message-format=short 2>&1 | tail -5
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```
Both must be clean. No tests added — this is pure plumbing for Tasks 4 + 5.

- [ ] **Step 4: Commit**

```
git add src/app.rs
git commit -m "feat(app): pending_edit field signals edit intent to run loop

The modal handler can't suspend the terminal directly (the handle
lives in run()), so it sets pending_edit instead. Task 5 wires
the run loop to detect this flag and perform the suspend / edit /
resume / save cycle."
```

---

### Task 4: `s` keybind + full modal handler

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the `s` arm in `handle_key_dashboard`**

After the existing `(KeyCode::Char('k'), _)` arm (process modal), insert:

```rust
        (KeyCode::Char('s'), _) => {
            let repo_id = match app.selected_target() {
                Some(SelectionTarget::Repo(id)) => Some(id),
                Some(SelectionTarget::Workspace(wid)) => app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == wid)
                    .map(|(rid, _)| *rid),
                None => app.repos.first().map(|r| r.id),
            };
            if let Some(id) = repo_id {
                app.modal = Some(Modal::RepoSettings {
                    repo_id: id,
                    selected: 0,
                });
            }
        }
```

- [ ] **Step 2: Replace the placeholder modal handler**

Find the placeholder `Modal::RepoSettings { .. }` arm added in Task 2 and replace with:

```rust
        Modal::RepoSettings {
            repo_id,
            mut selected,
        } => {
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    app.modal = Some(Modal::RepoSettings {
                        repo_id,
                        selected,
                    });
                }
                KeyCode::Down => {
                    let max = RepoSettingField::ALL.len() - 1;
                    selected = (selected + 1).min(max);
                    app.modal = Some(Modal::RepoSettings {
                        repo_id,
                        selected,
                    });
                }
                KeyCode::Enter => {
                    let field = RepoSettingField::ALL[selected.min(3)];
                    app.pending_edit = Some(PendingEdit { repo_id, field });
                    app.modal = None;
                }
                KeyCode::Char('d') => {
                    let field = RepoSettingField::ALL[selected.min(3)];
                    let _ = apply_repo_setting(app, repo_id, field, "");
                    let _ = app.refresh();
                    app.modal = Some(Modal::RepoSettings {
                        repo_id,
                        selected,
                    });
                }
                _ => {}
            }
        }
```

- [ ] **Step 3: Add the `apply_repo_setting` dispatcher**

Near `build_spawn_info` (or wherever fits in `src/app.rs`), add:

```rust
fn apply_repo_setting(
    app: &mut App,
    repo_id: crate::store::RepoId,
    field: RepoSettingField,
    value: &str,
) -> Result<()> {
    let trimmed = value.trim();
    let opt = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    match field {
        RepoSettingField::BranchPrefix => app.store.set_repo_branch_prefix(repo_id, trimmed),
        RepoSettingField::CustomInstructions => {
            app.store.set_repo_custom_instructions(repo_id, opt)
        }
        RepoSettingField::SetupScript => app.store.set_repo_setup_script(repo_id, opt),
        RepoSettingField::ArchiveScript => app.store.set_repo_archive_script(repo_id, opt),
    }
}
```

- [ ] **Step 4: Build + run tests**

```
cargo fmt
cargo build --message-format=short 2>&1 | tail -5
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```
All must be clean.

- [ ] **Step 5: Commit**

```
git add src/app.rs
git commit -m "feat(ui): s opens repo settings; modal handles edit/clear/nav

Dashboard 's' on any row opens Modal::RepoSettings for the
selected repo (or the parent repo when a workspace is selected).
Inside the modal: arrow keys navigate, Enter signals an edit
intent (run loop picks it up in Task 5), 'd' clears the highlighted
field directly. The editor invocation is wired in Task 5; for now
Enter closes the modal without doing anything visible."
```

---

### Task 5: Run-loop integration — suspend + edit + resume + save

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add `do_pending_edit` helper**

Above the `run` function (or wherever fits), add:

```rust
async fn do_pending_edit<B>(
    terminal: &mut ratatui::Terminal<B>,
    app: &SharedApp,
    edit: PendingEdit,
) -> Result<()>
where
    B: ratatui::backend::Backend + std::io::Write,
{
    // Read current value + extension hint under the lock.
    let (current, ext) = {
        let g = app.lock().await;
        let Some(repo) = g.repos.iter().find(|r| r.id == edit.repo_id) else {
            return Ok(());
        };
        match edit.field {
            RepoSettingField::BranchPrefix => (repo.branch_prefix.clone(), "txt"),
            RepoSettingField::CustomInstructions => {
                (repo.custom_instructions.clone().unwrap_or_default(), "md")
            }
            RepoSettingField::SetupScript => {
                (repo.setup_script.clone().unwrap_or_default(), "sh")
            }
            RepoSettingField::ArchiveScript => {
                (repo.archive_script.clone().unwrap_or_default(), "sh")
            }
        }
    };

    // Suspend the TUI.
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;

    let result = crate::external::edit_in_editor(&current, ext);

    // Resume the TUI.
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen
    )?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    if let Ok(Some(new)) = result {
        if new.trim() != current.trim() {
            let mut g = app.lock().await;
            let _ = apply_repo_setting(&mut g, edit.repo_id, edit.field, &new);
            let _ = g.refresh();
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Hook the run loop**

In `pub async fn run<B>(...)`, BEFORE the existing `terminal.draw(...)` call (inside the loop), insert:

```rust
        // Handle any pending edit BEFORE drawing — the editor takes
        // over the terminal and we need a clean redraw after it exits.
        let pending = {
            let mut g = app.lock().await;
            g.pending_edit.take()
        };
        if let Some(edit) = pending {
            do_pending_edit(terminal, &app, edit).await?;
        }
```

The existing draw + select! stay unchanged.

- [ ] **Step 3: Add a `B: Write` constraint on `run`**

`Terminal::backend_mut()` returns `&mut B`. To call `crossterm::execute!` against it, `B` needs `std::io::Write`. The `CrosstermBackend<Stdout>` already implements `Write`. Update the signature:

```rust
pub async fn run<B: Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: SharedApp,
) -> Result<()> { ... }
```

If the build complains about `TestBackend` not implementing `Write` (it does for `Display` writes, but not necessarily `std::io::Write`), check the existing tests that call `run` — if any do, scope the trait bound carefully. Most likely no tests call the full `run` (they call `handle_event` or `handle_key_*` directly), so this is fine.

- [ ] **Step 4: Build + verify**

```
cargo fmt
cargo build --message-format=short 2>&1 | tail -10
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```
All must be clean.

- [ ] **Step 5: Commit**

```
git add src/app.rs
git commit -m "feat(app): run loop handles pending_edit via TUI suspend/resume

Before each draw, the run loop checks for a pending edit, suspends
raw mode + alternate screen, runs \$EDITOR via external::edit_in_editor,
restores the TUI, and saves the changed value if non-empty and
different. Cancel paths (editor non-zero exit, unchanged content) leave
the store untouched."
```

---

### Task 6: Footer + README + commit spec/plan + push

**Files:**
- Modify: `src/ui/dashboard.rs` (footer)
- Modify: `README.md`
- Commit (untracked docs already on disk): `docs/superpowers/specs/2026-05-15-repo-settings-editor-design.md`, `docs/superpowers/plans/2026-05-15-repo-settings-editor.md`

- [ ] **Step 1: Dashboard footer**

Find the dashboard footer Paragraph. Insert `[s] settings` between `[k] procs` and `[d] archive`:

```rust
"[↑/↓] move   [enter] attach   [n] new   [N] new (YOLO)   [e] edit   [t] terminal   [v] diff   [k] procs   [s] settings   [d] archive   [q] quit"
```

- [ ] **Step 2: README keybinds table**

In the Dashboard keybinds table, add after the `[k]` row:

```
| `s` | Open repo settings modal for the selected repo (or the parent repo when a workspace is selected) |
```

- [ ] **Step 3: README — new subsection**

Under the existing "Per-repo setup scripts" section (search for `## Per-repo setup scripts`), add a new subsection:

```markdown
### Editing in the TUI

Press `s` on any dashboard row to open the Repo settings modal for that
row's repo. The modal lists the four per-repo fields:

- `branch_prefix`
- `custom_instructions`
- `setup_script`
- `archive_script`

`↑/↓` selects a field. Press `Enter` to edit — wsx temporarily leaves
the TUI, opens `$EDITOR` (or `vi` if unset) on a tempfile prepopulated
with the current value, and saves whatever you write when the editor
exits. Press `d` to clear the highlighted field. `Esc` closes.

The editor needs to be a terminal-native editor that returns when you
quit (vim, nvim, helix, micro, nano). GUI editors that return
immediately without a `--wait` flag will appear to "save nothing" —
keep `$EDITOR` pointed at a CLI editor for this flow.
```

- [ ] **Step 4: Verify everything**

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib -- --test-threads=1
```
All three must pass.

- [ ] **Step 5: Commit README + footer**

```
git add src/ui/dashboard.rs README.md
git commit -m "$(cat <<'EOF'
docs: repo settings TUI editor + [s] footer hint

Adds the Editing in the TUI subsection, the [s] keybind row in
the dashboard table, and the [s] settings entry in the footer.

Closes #24.
EOF
)"
```

- [ ] **Step 6: Commit spec + plan**

```
git add docs/superpowers/specs/2026-05-15-repo-settings-editor-design.md docs/superpowers/plans/2026-05-15-repo-settings-editor.md
git commit -m "docs: spec + plan for repo settings TUI editor"
```

- [ ] **Step 7: Push and confirm issue closure**

```
git push origin main
sleep 3
gh issue view 24 --json state,closedAt,stateReason
```
Expected: `"state":"CLOSED"`, `"stateReason":"COMPLETED"`.

---

## Self-review checklist

- [x] `edit_in_editor` happy path (returns content) AND cancel path (returns None) both tested
- [x] `EDITOR` env var save/restore in tests so they don't leak global state
- [x] `RepoSettingField` lives in `app.rs` and is referenced cross-module via `crate::app::RepoSettingField`
- [x] Modal::RepoSettings follows the live-state dispatch pattern (early-return in render(); draw() reads from app.repos directly)
- [x] `s` arm resolves repo_id from Repo / Workspace / None selections (matches existing `n`/`k` patterns)
- [x] Modal handler: Up/Down clamp; Enter signals via pending_edit + closes modal; `d` clears immediately + reopens modal; Esc closes
- [x] Run loop checks pending_edit BEFORE draw so the editor takes over the terminal before any wsx draw fights it
- [x] Suspend = disable_raw_mode + LeaveAlternateScreen; Resume = EnterAlternateScreen + enable_raw_mode + clear (in that order)
- [x] Trimmed empty save → clears the setting; trimmed unchanged → no DB write
- [x] Editor exit non-zero → cancel (no save, no error)
- [x] `B: Write` trait bound added so crossterm::execute! works on backend_mut
- [x] Footer entry + README keybinds table + README "Editing in the TUI" subsection all updated
- [x] No placeholders, no TBDs
