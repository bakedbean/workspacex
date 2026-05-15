# Repo settings editor in TUI — Design

**Issue:** [#24](https://github.com/bakedbean/workspacex/issues/24)

## Goal

Let the user view and edit per-repo settings (`branch_prefix`, `custom_instructions`, `setup_script`, `archive_script`) from inside the TUI, without dropping out to the CLI. Copy/paste must work for setup scripts and instructions.

## Approach

A new `Modal::RepoSettings` lists the four per-repo fields with their current values. Pressing Enter on a row suspends wsx (leaves alternate screen, disables raw mode), spawns `$EDITOR` on a temp file prepopulated with the current value, then resumes wsx after the editor exits and saves whatever the user wrote. This mirrors the existing CLI `wsx config edit <key>` flow — same pattern, TUI-triggered. `d` clears the selected field; `Esc` closes the modal.

No inline text input. All editing happens in the user's familiar editor, which already handles copy/paste, multi-line content, search, syntax highlighting, etc.

## Decisions

- **Keybind:** `s` on the dashboard (any row). On a Repo header, targets that repo. On a Workspace row, walks up to the parent repo. Mnemonic: "settings."
- **Modal layout:**
  ```
  ─── Repo settings — <repo-name> ───
    branch_prefix         <value or (unset)>
    custom_instructions   <preview or (unset)>
    setup_script          <preview or (unset)>
    archive_script        <preview or (unset)>
  ──────────────────────────────────
  [↑/↓] move   [enter] edit   [d] clear   [esc] close
  ```
  Preview: first 60 chars of the value (first line only for multi-line fields), with `…` appended when truncated. `(unset)` rendered in dim style when the value is `None` / empty.
- **Editor invocation:** `$EDITOR` env var; fall back to `vi`. Tempfile path: `$XDG_RUNTIME_DIR/wsx-edit-<random>.{ext}` or `/tmp/wsx-edit-<random>.{ext}` if unset. Extension hints (`.sh` for scripts, `.md` for instructions) help editors pick the right syntax mode.
- **Suspend/resume:** wsx leaves the alternate screen + disables raw mode → `Command::status()` blocks on the editor → wsx restores. Pattern follows the well-trodden ratatui+crossterm recipe; the implementation lives in the `run` loop where the `Terminal` handle is available (the modal key handler signals intent via a new `App.pending_edit` flag rather than calling the editor directly — keeps concerns separated).
- **Empty save = clear:** if the editor writes an empty file (after trim), the setting is cleared (`None` for nullable fields, `""` for `branch_prefix`). Matches the CLI's `wsx config set <key> ""` semantics.
- **Unchanged save = no-op:** if the new value equals the current value byte-for-byte, no DB write happens.
- **Editor exit non-zero = cancel:** treat as "user changed their mind." No save, no error modal, just resume the TUI.
- **Direct to main.** Functional feature, not subjective.

## Scope

### In

1. **New types** in `src/app.rs`:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum RepoSettingField {
       BranchPrefix,
       CustomInstructions,
       SetupScript,
       ArchiveScript,
   }
   #[derive(Debug, Clone)]
   pub struct PendingEdit {
       pub repo_id: crate::store::RepoId,
       pub field: RepoSettingField,
   }
   ```
2. **New `App` field:** `pub pending_edit: Option<PendingEdit>`. Initialized to `None`.
3. **New `Modal::RepoSettings { repo_id, selected }`** variant in `src/ui/modal.rs`.
4. **New `render_repo_settings(...)`** function in `src/ui/modal.rs` (follows the live-state `UpdatesPanel` / `ProcessList` dispatch pattern).
5. **`draw()` dispatch** in `src/app.rs` for the new variant.
6. **Modal key handler** in `handle_key_modal`:
   - `Up`/`Down` clamp-navigate `selected` in `0..4`.
   - `Enter` sets `app.pending_edit = Some(PendingEdit { repo_id, field })` for the highlighted row, then closes the modal. (v1 closes the modal after one edit. If the user wants to edit another field, they re-press `s`. Reopening the modal automatically after each edit is a deferred follow-up.)
   - `d` clears the highlighted field via the appropriate `store.set_repo_*` call with `None` / `""`.
   - `Esc` closes the modal.
7. **`s` keybind** in `handle_key_dashboard`:
   - On `SelectionTarget::Repo(id)` → open `Modal::RepoSettings { repo_id: id, selected: 0 }`.
   - On `SelectionTarget::Workspace(wid)` → walk to the parent repo via `app.workspaces`, then open the modal.
8. **Run-loop handling** of `app.pending_edit` (in `src/app.rs::run`):
   - Before each `terminal.draw`, check `app.pending_edit.take()`.
   - If `Some(edit)`: read current value from the Repo struct, suspend the TUI, call `external::edit_in_editor`, resume the TUI, save if changed.
9. **`external::edit_in_editor(initial: &str, ext_hint: &str) -> Result<Option<String>>`** — writes tempfile, spawns `$EDITOR`, reads back, returns `Ok(Some(new))` on success with content, `Ok(None)` on editor non-zero exit, `Err` only for I/O failure. Uses `tempfile` crate (already a dev-dep — promote to a regular dep if needed) or rolls its own via `std::env::temp_dir()` + random suffix.
10. **Footer:** dashboard adds `[s] settings`.
11. **README:** new "Per-repo settings (TUI)" subsection under the existing "Per-repo setup scripts" section, listing the keybind, the editor flow, and the four fields. Updates the keybinds table.
12. **Tests:**
    - Modal renderer: shows current values, shows "(unset)" for `None` fields, truncates long values.
    - Field navigation: `Up`/`Down` clamping.
    - `d` clears: setting a value via `store`, opening modal, pressing `d`, verifying DB is cleared.
    - `external::edit_in_editor` happy path: invoke with `$EDITOR=/bin/true` (which exits 0 and doesn't modify the file) → returns the initial content (unchanged). Empty initial + `/bin/true` → returns empty string.
    - `external::edit_in_editor` cancel path: `$EDITOR=/bin/false` (exits 1) → returns `Ok(None)`.

### Out

- Inline text input (chose suspend-to-editor).
- Per-setting per-repo overrides not in the existing store schema (the four fields above are it).
- Editing repo path / name (those are creation-time identity).
- Multi-buffer / multi-file editing.
- Diff preview before save.
- Help text / setting documentation inside the modal.
- "Set across all repos" bulk operation.
- Setting validation (e.g., shell syntax check on setup_script).

## Implementation notes

### `external::edit_in_editor`

```rust
pub fn edit_in_editor(initial: &str, ext_hint: &str) -> Result<Option<String>> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let dir = std::env::temp_dir();
    let suffix: u64 = rand_simple(); // tiny non-crypto random; SystemTime-based is fine
    let path = dir.join(format!("wsx-edit-{suffix}.{ext_hint}"));
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

(Tempfile suffix: use `std::process::id()` combined with `SystemTime::now().duration_since(UNIX_EPOCH).as_nanos()`. No cryptographic strength needed — we just want collision avoidance against concurrent wsx invocations.)

### Suspend/resume in `run`

```rust
loop {
    // Handle pending edit BEFORE drawing.
    let pending = {
        let mut g = app.lock().await;
        g.pending_edit.take()
    };
    if let Some(edit) = pending {
        do_pending_edit(terminal, &app, edit).await?;
    }

    {
        let mut g = app.lock().await;
        terminal.draw(|f| draw(f, &mut g))?;
        if g.quit {
            break;
        }
    }
    // ... existing select! ...
}
```

`do_pending_edit`:

```rust
async fn do_pending_edit<B: Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: &SharedApp,
    edit: PendingEdit,
) -> Result<()> {
    let (current, ext) = {
        let g = app.lock().await;
        let repo = g.repos.iter().find(|r| r.id == edit.repo_id);
        let (val, ext) = match (repo, edit.field) {
            (Some(r), RepoSettingField::BranchPrefix) => (r.branch_prefix.clone(), "txt"),
            (Some(r), RepoSettingField::CustomInstructions) => {
                (r.custom_instructions.clone().unwrap_or_default(), "md")
            }
            (Some(r), RepoSettingField::SetupScript) => {
                (r.setup_script.clone().unwrap_or_default(), "sh")
            }
            (Some(r), RepoSettingField::ArchiveScript) => {
                (r.archive_script.clone().unwrap_or_default(), "sh")
            }
            (None, _) => return Ok(()),
        };
        (val, ext)
    };

    // Suspend TUI.
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;

    let result = crate::external::edit_in_editor(&current, ext);

    // Resume TUI.
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::EnterAlternateScreen)?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    if let Ok(Some(new)) = result {
        let new_trimmed = new.trim().to_string();
        if new_trimmed != current.trim() {
            let mut g = app.lock().await;
            apply_repo_setting(&mut g, edit.repo_id, edit.field, &new_trimmed)?;
            let _ = g.refresh();
        }
    }
    Ok(())
}

fn apply_repo_setting(
    g: &mut App,
    repo_id: RepoId,
    field: RepoSettingField,
    value: &str,
) -> Result<()> {
    let opt = if value.is_empty() { None } else { Some(value) };
    match field {
        RepoSettingField::BranchPrefix => g.store.set_repo_branch_prefix(repo_id, value),
        RepoSettingField::CustomInstructions => g.store.set_repo_custom_instructions(repo_id, opt),
        RepoSettingField::SetupScript => g.store.set_repo_setup_script(repo_id, opt),
        RepoSettingField::ArchiveScript => g.store.set_repo_archive_script(repo_id, opt),
    }
}
```

Note: `set_repo_branch_prefix` takes a `&str` (empty = unset), the others take `Option<&str>`. The wrapper normalizes.

### Modal renderer details

```rust
pub fn render_repo_settings(
    f: &mut Frame,
    area: Rect,
    repo_name: &str,
    fields: &[(RepoSettingField, &str)],  // (field, current display value)
    selected: usize,
    theme: &Theme,
) { ... }
```

Field labels (left column): exact names from the spec (`branch_prefix`, `custom_instructions`, `setup_script`, `archive_script`). Value column: first non-empty line trimmed, truncated at 60 chars with `…`, or `(unset)` in dim style.

### Repo lookup from selection

In `handle_key_dashboard`, the `s` arm needs to resolve a `RepoId` from the current selection. The pattern is already used by `n` (new workspace) and others:

```rust
let repo_id = match app.selected_target() {
    Some(SelectionTarget::Repo(id)) => Some(id),
    Some(SelectionTarget::Workspace(wid)) => app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == wid)
        .map(|(rid, _)| *rid),
    None => None,
};
```

## Risks

- **Suspend/resume terminal bugs.** Some terminals don't restore cursor/style cleanly on EnterAlternateScreen. The `terminal.clear()` after resume mitigates by forcing a full redraw. If users see artifacts, that's the place to add explicit cursor-show + style-reset.
- **`$EDITOR` not found.** Falls back to `vi`. If `vi` is also missing (rare on Linux/macOS), the spawn errors and we surface an error modal — acceptable, single-user setup.
- **User opens a GUI editor with `$EDITOR=code`.** Modal editors (code, sublime) typically wait via `--wait`. wsx waits on the editor's process; if `code` is set as `$EDITOR` directly, it likely returns immediately and we save the unchanged tempfile. Documented in the README as a footgun; recommend a CLI editor (vim, nvim, helix, micro, nano) for this flow.
- **Race: user edits a setting that branch_drift_poll is reading.** The poll loop reads `App.repos` under the lock; we update under the lock too. No race.
- **Tempfile cleanup.** Best-effort `remove_file` after read. If wsx crashes mid-edit, a stray `wsx-edit-*.{sh,md,txt}` file is left in `/tmp` (acceptable; systems sweep `/tmp`).

## Out-of-scope follow-ups

- A way to view setting source (which file, which CLI command set it) — not knowable from the DB today.
- Setting history / undo.
- "Settings → migrate from `.claudette.json`" wizard (if anyone still has legacy state).
- Inline single-line editing for `branch_prefix` as a fast-path (skip the editor for short fields). Could ship later if the round-trip-to-editor feels heavy.
