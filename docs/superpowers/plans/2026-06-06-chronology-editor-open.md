# Chronology Editor Open (config-driven) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the chronology "open change at line" action require a configured `editor_cmd` (no silent `$EDITOR` fallback), inject the file+line by detecting the editor anywhere in the command (so window-wrapper commands keep the line), and surface every failure as a visible modal.

**Architecture:** Two pure helpers in `src/commands/external.rs` — an upgraded `resolve_editor_at_argv` that scans all command tokens for a known editor (not just the first), and a small `editor_open_decision` that gates launch on a non-empty `editor_cmd`. A single `open_focused_change` helper in `src/app/input.rs` consumes both, replacing the two duplicated open sites and their silent `tracing::warn!` with `Modal::Error` surfacing.

**Tech Stack:** Rust, `shlex` (command parsing), `ratatui`/`crossterm` (the modal is existing UI). Tests are `#[cfg(test)]` unit tests via `cargo test`.

**Builds on (verified current code):**
- `src/commands/external.rs`: `fn resolve_editor_at_argv(cmd: &str, file: &str, line: u32) -> Result<Vec<String>>` currently inspects only the FIRST token's basename for the goto fallback (`code`/`codium`/`cursor` → `--goto file:line`; `vim`/`nvim`/`vi`/`emacs`/`emacsclient` → `+line file`; else append file); `pub fn open_in_editor_at(worktree, file, line, configured) -> Result<()>` resolves via `resolve_editor_cmd(configured)` (which falls back to `$VISUAL`/`$EDITOR` only when `configured` is `None`/empty) and spawns detached. `Error`/`Result` are `crate::error::{Error, Result}`.
- `src/app/input.rs`: the keyboard `NavAction::Open(i)` arm and the mouse expanded-detail-click branch each inline the same block: resolve focused workspace + event (clone `worktree`/`file_path`/`detail`), `resolve_line_in_file`, read `editor_cmd` via `app.store.get_setting("editor_cmd").ok().flatten()`, call `open_in_editor_at(&worktree, &file, line, editor.as_deref())`, and on `Err` only `tracing::warn!`. `focused_attached_workspace(app) -> Option<(WorkspaceId, PathBuf)>` exists. `Modal` is already imported (used for `Modal::Error` elsewhere in the file).
- `src/ui/modal.rs`: `Modal::Error { message: String }` exists and renders after the view match (so it shows over the attached view) and is dismissible by the existing input handler.

---

## File Structure

- `src/commands/external.rs` (modify) — `GotoStyle` enum, `known_editor_goto`, upgraded `resolve_editor_at_argv`, `EditorOpenDecision` enum, `editor_open_decision` + tests.
- `src/app/input.rs` (modify) — `open_focused_change` helper; keyboard + mouse open sites call it; remove the duplicated blocks and silent warns.
- `README.md` (modify) — document the `editor_cmd` requirement and file+line injection for the chronology open.

---

## Task 1: Scan all tokens for the editor in `resolve_editor_at_argv`

**Files:**
- Modify: `src/commands/external.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/commands/external.rs` (alongside the existing `editor_at_*` tests):

```rust
#[test]
fn editor_at_wrapper_terminal_editor_keeps_line() {
    // window-wrapper: the inner editor (nvim) must be detected, not alacritty
    let argv = resolve_editor_at_argv("alacritty -e nvim", "/wt/a.rs", 42).unwrap();
    assert_eq!(argv, vec!["alacritty", "-e", "nvim", "+42", "/wt/a.rs"]);
}

#[test]
fn editor_at_wrapper_gui_editor_uses_goto() {
    let argv = resolve_editor_at_argv("wezterm start -- code", "/wt/a.rs", 7).unwrap();
    assert_eq!(argv, vec!["wezterm", "start", "--", "code", "--goto", "/wt/a.rs:7"]);
}

#[test]
fn editor_at_zed_uses_goto() {
    let argv = resolve_editor_at_argv("zed", "/wt/a.rs", 5).unwrap();
    assert_eq!(argv, vec!["zed", "--goto", "/wt/a.rs:5"]);
}

#[test]
fn editor_at_nano_uses_plus_line() {
    let argv = resolve_editor_at_argv("nano", "/wt/a.rs", 5).unwrap();
    assert_eq!(argv, vec!["nano", "+5", "/wt/a.rs"]);
}
```

(The existing tests — `editor_at_substitutes_file_and_line_placeholders`, `editor_at_vim_fallback_uses_plus_line`, `editor_at_code_fallback_uses_goto`, `editor_at_emacs_fallback_uses_plus_line`, `editor_at_unknown_editor_appends_file_only`, `editor_at_substitutes_placeholders_in_separate_tokens` — must continue to pass unchanged: they are the bare-editor / placeholder / unknown cases the rewrite still handles.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib editor_at_wrapper`
Expected: FAIL — `alacritty -e nvim` currently appends only the file (first-token `alacritty` isn't a known editor), so the line `+42` is missing.

- [ ] **Step 3: Rewrite `resolve_editor_at_argv` to scan all tokens**

Replace the existing `resolve_editor_at_argv` in `src/commands/external.rs` with:

```rust
/// How an editor wants a file+line on its command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GotoStyle {
    /// VS Code family: `--goto file:line`.
    Goto,
    /// vi/emacs family: `+line file`.
    PlusLine,
}

/// Map an editor program basename to its goto style, if known.
fn known_editor_goto(basename: &str) -> Option<GotoStyle> {
    match basename {
        "code" | "codium" | "cursor" | "zed" => Some(GotoStyle::Goto),
        "vim" | "nvim" | "vi" | "nano" | "emacs" | "emacsclient" => Some(GotoStyle::PlusLine),
        _ => None,
    }
}

/// Resolve the editor command into argv that opens `file` at `line`.
///
/// Resolution order:
/// 1. If the command contains `{file}`/`{line}` placeholders, substitute them.
/// 2. Else scan ALL tokens for the first one whose basename is a known editor
///    and append that editor's goto syntax (so window-wrapper commands like
///    `alacritty -e nvim` detect the inner editor and keep the line).
/// 3. Else append the file (line dropped); the user can add `{file}`/`{line}`
///    placeholders for an unrecognized editor.
fn resolve_editor_at_argv(cmd: &str, file: &str, line: u32) -> Result<Vec<String>> {
    let line_s = line.to_string();
    let mut parts = shlex::split(cmd)
        .ok_or_else(|| Error::UserInput(format!("could not parse command: {cmd}")))?;
    if parts.is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    let used_placeholder = parts
        .iter()
        .any(|p| p.contains("{file}") || p.contains("{line}"));
    if used_placeholder {
        for part in &mut parts {
            *part = part.replace("{file}", file).replace("{line}", &line_s);
        }
        return Ok(parts);
    }
    let style = parts.iter().find_map(|p| {
        let base = std::path::Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(p);
        known_editor_goto(base)
    });
    match style {
        Some(GotoStyle::Goto) => {
            parts.push("--goto".to_string());
            parts.push(format!("{file}:{line_s}"));
        }
        Some(GotoStyle::PlusLine) => {
            parts.push(format!("+{line_s}"));
            parts.push(file.to_string());
        }
        None => parts.push(file.to_string()),
    }
    Ok(parts)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib editor_at_`
Expected: PASS — the 4 new tests plus all pre-existing `editor_at_*` tests.

- [ ] **Step 5: Commit**

```bash
git add src/commands/external.rs
git commit -m "feat(editor): scan all tokens for the editor so wrappers keep the line"
```

---

## Task 2: `editor_open_decision` — require a configured editor

**Files:**
- Modify: `src/commands/external.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/commands/external.rs`:

```rust
#[test]
fn editor_decision_needs_config_when_unset_or_blank() {
    assert_eq!(editor_open_decision(None), EditorOpenDecision::NeedsConfig);
    assert_eq!(editor_open_decision(Some("")), EditorOpenDecision::NeedsConfig);
    assert_eq!(editor_open_decision(Some("   ")), EditorOpenDecision::NeedsConfig);
}

#[test]
fn editor_decision_launches_trimmed_command() {
    assert_eq!(
        editor_open_decision(Some("  alacritty -e nvim  ")),
        EditorOpenDecision::Launch("alacritty -e nvim".to_string())
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib editor_decision`
Expected: FAIL — `editor_open_decision` / `EditorOpenDecision` not found.

- [ ] **Step 3: Implement**

Add to `src/commands/external.rs` (non-test scope):

```rust
/// Outcome of deciding whether the chronology open-at-line can launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorOpenDecision {
    /// Launch the trimmed command.
    Launch(String),
    /// No usable `editor_cmd`; the caller should prompt the user to configure one.
    NeedsConfig,
}

/// Decide whether the chronology open-at-line can launch. A non-empty,
/// non-whitespace `editor_cmd` yields `Launch`; anything else `NeedsConfig`.
/// Unlike `open_in_editor`, this path does NOT fall back to `$VISUAL`/`$EDITOR`
/// — opening a file at a line needs an editor the user has chosen to wire up.
pub fn editor_open_decision(editor_cmd: Option<&str>) -> EditorOpenDecision {
    match editor_cmd {
        Some(c) if !c.trim().is_empty() => EditorOpenDecision::Launch(c.trim().to_string()),
        _ => EditorOpenDecision::NeedsConfig,
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib editor_decision`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/commands/external.rs
git commit -m "feat(editor): editor_open_decision gates open-at-line on configured editor_cmd"
```

---

## Task 3: `open_focused_change` helper + wire both open sites

**Files:**
- Modify: `src/app/input.rs`

This task has no isolated unit test (it sets `Modal::Error` / spawns — side effects); the decision + argv logic it depends on are unit-tested in Tasks 1–2. Verified by build + manual.

- [ ] **Step 1: Add the helper**

Add to `src/app/input.rs` (near `focused_attached_workspace`):

```rust
/// Open the chronology entry at `idx` in the user's configured editor at the
/// changed line. Requires `editor_cmd` (no `$EDITOR` fallback for this path):
/// surfaces a `Modal::Error` when it's unset or when the spawn fails.
fn open_focused_change(app: &mut App, idx: usize) {
    use crate::commands::external::{EditorOpenDecision, editor_open_decision};
    // Clone the path + detail out of the chronology borrow before touching
    // app.store / app.modal.
    let Some((worktree, file, detail)) =
        focused_attached_workspace(app).and_then(|(ws_id, worktree)| {
            app.chronology.get(&ws_id).and_then(|t| {
                t.events()
                    .get(idx)
                    .map(|ev| (worktree, ev.file_path.clone(), ev.detail.clone()))
            })
        })
    else {
        return;
    };
    let editor_cmd = app.store.get_setting("editor_cmd").ok().flatten();
    match editor_open_decision(editor_cmd.as_deref()) {
        EditorOpenDecision::NeedsConfig => {
            app.modal = Some(crate::ui::modal::Modal::Error {
                message: "No editor_cmd configured. Set one to open changes in your \
                          editor, e.g.\n  wsx config set editor_cmd 'alacritty -e nvim'"
                    .to_string(),
            });
        }
        EditorOpenDecision::Launch(cmd) => {
            let line = crate::activity::chronology::resolve_line_in_file(&file, &detail);
            if let Err(e) =
                crate::commands::external::open_in_editor_at(&worktree, &file, line, Some(&cmd))
            {
                app.modal = Some(crate::ui::modal::Modal::Error {
                    message: format!("Failed to open editor: {e}"),
                });
            }
        }
    }
}
```

- [ ] **Step 2: Wire the keyboard open site**

In the chronology key-interception block, replace the entire `NavAction::Open(i) => { ... }` arm body (the block that currently clones the event, calls `open_in_editor_at`, and `tracing::warn!`s on error) with a single call:

```rust
                NavAction::Open(i) => open_focused_change(app, i),
```

- [ ] **Step 3: Wire the mouse open site**

In `handle_mouse`, the chronology detail-click branch currently inlines the same open block then sets focus/sel. Replace its inline open block (the `let target = ...; if let Some((worktree, file, detail)) = target { ... tracing::warn! ... }`) with a call to the helper, keeping the focus/sel lines:

```rust
            }) {
                open_focused_change(app, idx);
                app.chronology_focused = true;
                app.chronology_sel = crate::ui::chronology_nav::ChronoSel::Detail(idx);
            } else if let Some(idx) = app.chronology_entry_rects.iter().find_map(|(i, r)| {
                // ... existing header-click branch, unchanged ...
```

Read the actual branch and replace only the open block; preserve the `app.chronology_focused = true;` / `app.chronology_sel = ChronoSel::Detail(idx);` lines and the rest of the click chain.

- [ ] **Step 4: Build + manual verify**

Run: `cargo build` — ZERO warnings (the old `tracing::warn!`-only blocks are gone; confirm no now-unused imports remain — if `tracing` is still used elsewhere in the file it stays, otherwise remove the unused import).
Run: `cargo test --lib` — no regressions (~1118).
Manual: in an attached Claude workspace with `editor_cmd` unset, trigger a chronology open (Enter on a detail, or click the expanded detail) → a dismissible error modal appears with the configure hint. Then `wsx config set editor_cmd 'alacritty -e nvim'` and repeat → nvim opens in a new alacritty window at the change's line.

- [ ] **Step 5: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(chronology): require editor_cmd for open; surface failures as a modal"
```

---

## Task 4: README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the editor open behavior**

In the "Change chronology" section of `README.md` (where opening a change in the editor is described), add/adjust prose to state:
- Opening a change at its line requires a configured `editor_cmd`; if unset, wsx shows a prompt to configure it (it does not fall back to `$EDITOR` for this action).
- wsx injects the file and line into `editor_cmd` at runtime: if the command contains `{file}`/`{line}` placeholders they are substituted; otherwise wsx detects a known editor anywhere in the command (`code`/`codium`/`cursor`/`zed`, or `vim`/`nvim`/`vi`/`nano`/`emacs`/`emacsclient`) and appends the right goto args — so a window wrapper like `alacritty -e nvim` opens the file at the line in its own window.
- Example: `wsx config set editor_cmd 'alacritty -e nvim'`.
- Unrecognized editors: add `{file}`/`{line}` placeholders to `editor_cmd` to control the syntax.

Match the README's existing prose/code-block style.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document chronology editor open (editor_cmd required + file:line injection)"
```

---

## Self-Review (completed during planning)

**Spec coverage:**
- No `editor_cmd` → visible warning, no `$EDITOR` fallback → Task 2 (`editor_open_decision`) + Task 3 (`NeedsConfig` → `Modal::Error`). ✓
- `editor_cmd` set → inject file+line at runtime, execute → Task 1 (`resolve_editor_at_argv` scan/append) + Task 3 (`Launch` → `open_in_editor_at(Some(cmd))`). ✓
- Scan all tokens (vs first-token) so wrappers keep the line → Task 1. ✓
- Single `editor_cmd` serves both dir-open and at-line (no separate setting) → Task 1 design (append at end), unchanged `e`/`Ctrl-x e`. ✓
- Surface spawn failures visibly → Task 3 (`Err` → `Modal::Error`). ✓
- Placeholders remain an escape hatch → Task 1 (placeholder branch first). ✓
- Scope: chronology open only; `e`/`Ctrl-x e` untouched → Tasks 3 only touches the two chronology open sites. ✓
- Tests: pure `resolve_editor_at_argv` + `editor_open_decision` → Tasks 1–2. ✓
- README → Task 4. ✓

**Placeholder scan:** No "TBD"/vague steps; every code step shows complete code. Task 3's manual-verify is explicit. The only non-code instruction (Task 3 Step 3 "replace only the open block") names exactly what to preserve.

**Type consistency:** `GotoStyle` (`Goto`/`PlusLine`), `known_editor_goto`, `resolve_editor_at_argv` (unchanged signature), `EditorOpenDecision` (`Launch(String)`/`NeedsConfig`), `editor_open_decision`, `open_focused_change`, `open_in_editor_at(.., Some(&cmd))`, `Modal::Error { message }`, `ChronoSel::Detail` — names consistent across tasks.
