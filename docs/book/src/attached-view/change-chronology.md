When an agent is actively editing files, it's easy to lose track of what changed, where, and when — especially across a long session with many small edits. The change chronology bar is a toggleable vertical panel docked to the side of the **attached** view that rebuilds your spatial and temporal memory of what the agent touched.

The bar shows a newest-first, time-ordered list of individual file edits the agent made — one entry per change, not per commit. Each entry is a single line: the time and the file path. Long paths are abbreviated by collapsing the ancestor directories to their first letter, keeping the parent directory and filename readable (e.g. `docs/superpowers/specs/2026-06-05-foo.md` shows as `d/s/specs/2026-06-05-foo.md`). Press `Enter` on an entry (or click it) to open the **full-change detail modal**, a scrollable overlay showing the complete diff with a line-number gutter — added (`+`) lines are numbered with their current file line (the same line the editor opens to), while removed (`-`) lines show a blank gutter.

Currently the chronology is reconstructed from Claude Code's on-disk session logs. Support for other agents is added incrementally as those log formats are covered.

### Keyboard navigation

The chronology bar is a focusable pane. While attached, press `Ctrl-x` then an arrow key **toward the bar's side** to move keyboard focus into it (bar on the right → `Ctrl-x →`; bar on the left → `Ctrl-x ←`). This only works from the edge pane adjacent to the bar; otherwise `Ctrl-x`+arrow keeps moving between agent split panes as normal.

While the bar is focused, keystrokes are captured by the bar and do **not** reach the agent:

- `↑` / `k` and `↓` / `j` move the selection; `g` jumps to the top (newest), `G` to the bottom.
- `Enter` on an entry opens the full-change detail modal for that entry.
- `Esc` (or `Ctrl-x` + arrow **away** from the bar's side) returns focus to the agent pane.

### Detail modal

The modal is a scrollable overlay showing the full diff of the selected change:

- Scroll with `↑` / `↓`, `j` / `k`, `PgUp` / `PgDn`, `g` / `G`, or the mouse wheel.
- Press `e` to open the file in your editor at the changed line (requires `editor_cmd` — see below).
- Press `Esc` or click outside the modal to close it and return to the bar.

The diff is displayed with basic syntax highlighting for Rust, Python, Shell, and a generic C-like family (C/C++/JS/TS/Go/Java/JSON, and similar); other file types are shown plain. Added (`+`) lines are tinted green and removed (`-`) lines red; the line-number gutter stays dim. Highlighting is per-line — multi-line strings or block comments may not be perfectly colored.

### Keybindings (attached view, under the `Ctrl-x` leader)

| Key        | Action                                          |
| ---------- | ----------------------------------------------- |
| `Ctrl-x c` | Toggle the chronology bar on/off                |
| `Ctrl-x C` | Swap the bar's side (left ↔ right)              |

Mouse wheel over the bar scrolls it. Click an entry to focus the bar, select the entry, and open the detail modal.

### Opening a file at the changed line

Pressing `e` inside the detail modal opens the file in your editor, jumping directly to the modified line.

**`editor_cmd` is required for this action.** If `editor_cmd` is unset, wsx shows a dismissible prompt telling you to configure it. There is no silent fallback to `$VISUAL` or `$EDITOR` for this specific action — those env-var fallbacks still apply to the separate `[e]` / `Ctrl-x e` "open workspace in editor" actions, which are unchanged.

**File and line injection.** When `editor_cmd` is set, wsx injects the file path and line number at runtime using one of two strategies:

- **Placeholders**: if your command contains `{file}`, `{line}`, and/or `{path}`, they are substituted in place. `{path}` is the worktree root (the same value substituted by the `[e]` dir-open action), so a single `editor_cmd` works for both actions. Use placeholders for editors wsx doesn't recognize or when you need exact control over argument order.
- **Auto-detection**: if no `{file}` or `{line}` placeholders are present, wsx scans the command for a known editor name and appends the appropriate goto arguments (after substituting any `{path}` first):
  - `code`, `codium`, `cursor`, `zed` → `--goto <file>:<line>`
  - `vim`, `nvim`, `vi`, `nano`, `emacs`, `emacsclient` → `+<line> <file>`

Detection matches the editor name **anywhere** in the command, so a terminal wrapper works transparently. For example, `alacritty -e nvim` is detected as nvim and becomes `alacritty -e nvim +<line> <file>`, opening the file at the changed line in a new terminal window.

```bash
wsx config set editor_cmd 'alacritty -e nvim'
```

Commands with `{path}` also work — the worktree is substituted first, then the editor is auto-detected or `{file}`/`{line}` are substituted:

```bash
wsx config set editor_cmd 'xdg-terminal-exec --dir={path} nvim'
```

For an editor wsx doesn't recognize, add `{file}` and `{line}` placeholders to control the exact syntax:

```bash
wsx config set editor_cmd 'myed --line {line} {file}'
```

**Error visibility.** If the editor fails to launch, wsx surfaces the error in a dismissible prompt — failures are no longer silent.

### Schema and defaults

`chronology_config` is a JSON blob set globally via `wsx config set` or overridden per-repo via the repo settings modal (`s` on the dashboard, select the `chronology_config` row). Every field is optional; missing fields fall back to defaults.

| Field            | Type                  | Default  | Effect                                                                 |
| ---------------- | --------------------- | -------- | ---------------------------------------------------------------------- |
| `visible`        | bool                  | `true`   | Master toggle. `false` hides the bar entirely (same as `Ctrl-x c`).   |
| `side`           | `"left"` / `"right"`  | `"right"` | Which side of the attach area the bar is docked to.                   |
| `width.percent`  | u8                    | `32`     | Target width as a percent of the attach area's columns.                |
| `width.min_cols` | u16                   | `24`     | Minimum width in columns.                                              |
| `width.max_cols` | u16                   | `60`     | Maximum width in columns.                                              |

### Setting the global value

```bash
wsx config set chronology_config '{"side":"left","width":{"min_cols":30}}'
wsx config get chronology_config
wsx config set chronology_config ""   # clear (reverts to defaults)
```

Partial JSON is fine — unspecified fields inherit defaults. Malformed JSON is rejected with a non-zero exit and the previous value is preserved.

### Per-repo override

Open the repo settings modal with `s` on the dashboard, select the `chronology_config` row, and press Enter. `$EDITOR` opens on `{}\n` (or the current override). Save to apply; press `d` to clear the override and fall back to the global value.

Example — pin the bar to the left for a repo with a wide main pane:

```json
{ "side": "left", "width": { "percent": 28 } }
```
