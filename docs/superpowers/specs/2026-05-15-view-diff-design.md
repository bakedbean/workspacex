# View diff — Design

**Issue:** [#14](https://github.com/bakedbean/workspacex/issues/14)

## Goal

Add a one-keystroke way to see what's changed in the selected workspace versus the repo's main branch, by spawning the user's configured difftool against the worktree. Mirrors the existing `[e] open in editor` and `[t] open in terminal` pattern.

## Approach

Add `[v]` to the dashboard. When pressed with a workspace selected, wsx looks up the workspace's worktree path and resolves its repo's main branch, then spawns a detached process per the user's `diff_cmd` template. No in-app diff rendering — the user's chosen tool (delta + less, neovim Diffview, VS Code's diff, etc.) does the work.

## Decisions

- **Keybind:** `[v]` (view diff). Available on the dashboard when a workspace row is selected; no-op on a Repo header (matches `[d]`).
- **Configuration:** new `diff_cmd` setting in the DB settings table. No env-var fallback (no obvious analogue to `$EDITOR`). Unset → error modal with example commands.
- **Placeholders:** `{path}` (worktree path) and `{base}` (main branch name) substituted anywhere they appear. If neither is present, append `{path}` at the end (matches the editor-style append behavior).
- **Base branch detection:** at point of use, run `git symbolic-ref --short refs/remotes/origin/HEAD` inside the worktree. Strip `origin/` prefix. Fall back to `main` if that command fails (no remote, no origin/HEAD set). No persistence — recompute is ~1ms.
- **Spawn semantics:** detached, like editor/terminal. wsx stays running; user closes diff tool when done.
- **Footer hint:** add `[v] view diff` to the dashboard footer string.

## Scope

### In
1. New `open_diff(worktree, base, configured)` in `src/external.rs`.
2. Generalize `resolve_argv` to accept a `&[(name, value)]` substitution list instead of just `{path}`.
3. New `(KeyCode::Char('v'), _)` arm in the dashboard key handler.
4. New `resolve_base_branch(worktree) -> String` helper (in `src/git.rs` — fits with other git shellouts).
5. New `diff_cmd` setting (read via existing `store.get_setting`; no new column).
6. Dashboard footer updated.
7. README "Editor and terminal integration" section becomes "Editor, terminal, and diff integration" with a third subsection covering `diff_cmd` + examples.
8. Tests for: `resolve_argv` with the new substitution map; `resolve_base_branch` returns the detected branch when origin/HEAD is set, falls back to `main` when not; `open_diff` spawns successfully against a known good command.

### Out
- Per-repo override of the main-branch detection (defer until auto-detect demonstrably fails for someone).
- In-app diff rendering / pager (option B from brainstorm).
- Diff against anything other than the main branch (against upstream, parent branch, etc.).
- Caching the detected base on `Repo` — recompute every press is cheap.

## Implementation notes

### Substitution map generalization
Current:
```rust
fn resolve_argv(cmd: &str, path: &Path, append_when_no_placeholder: bool) -> Result<Vec<String>>
```

Generalize to:
```rust
fn resolve_argv(
    cmd: &str,
    substitutions: &[(&str, &str)],   // e.g. &[("path", "/tmp/wt"), ("base", "main")]
    append_path_when_no_placeholder: Option<&str>,  // None for terminal, Some(path) for editor/diff
) -> Result<Vec<String>>
```

Editor and terminal callers pass `&[("path", path_str)]`; diff passes both `path` and `base`. The "append a fallback if no placeholder used" stays single-purpose (still just the path).

### Base detection
```rust
pub async fn resolve_base_branch(worktree: &Path) -> String {
    // git symbolic-ref --short refs/remotes/origin/HEAD → "origin/main" → strip → "main"
    // any failure → "main"
}
```

Async to match the rest of `src/git.rs`. No error type — silent fallback to `main`.

### Footer length
Dashboard footer is already ~108 chars. Adding `[v] view diff` pushes it further. Acceptable — ratatui clips gracefully and the more-important keys are leftmost. If the footer becomes a real problem, a separate `[?] help` overlay is a clean follow-up.

## Risks

- **Auto-detect fails for some repo setups.** Repos without `origin` set, or with `origin/HEAD` not configured (a common gotcha after `git clone` on some setups), get the `main` fallback. If their main branch is actually `master`, diff target is wrong. Mitigation: clear error from the difftool ("unknown revision `main`"). User can either fix `git remote set-head origin --auto` or wait for the per-repo override follow-up.
- **`diff_cmd` complexity for users.** A correct invocation often needs a shell wrapper (`sh -c '...'`) because pipes / cwd / multiple commands. Documentation needs concrete examples covering the common cases (terminal-wrapped pager, Diffview, VS Code).
- **Spawn detached + interactive diff.** A pager-in-terminal `diff_cmd` like `alacritty -e ...` opens a new window. That's fine on desktop Linux. SSH-attached use isn't expected to work (no DISPLAY for new windows) — but that's identical to how `[e]` and `[t]` behave today and isn't a regression.

## Out-of-scope follow-ups (not commitments)

- Per-repo `main_branch` setting if auto-detect proves unreliable.
- In-app diff pane (View::Diff scrollable buffer).
- `[v]` on a workspace + selecting a diff range (vs main, vs upstream, vs HEAD~1).
