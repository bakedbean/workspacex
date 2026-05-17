# Pinned commands in the attached view — Design

**Issue:** [#37](https://github.com/bakedbean/workspacex/issues/37)

## Goal

Give the user a one-keystroke (or one-click) way to send a fixed set of frequently-used slash commands to the attached claude session — `/pull-request`, `/feedback`, `/ultrareview`, etc. Inspired by Claudette's pinned-commands UX, adapted to wsx's TUI conventions.

The user defines their own list globally; per-repo overrides allow a repo to swap in a different list when its workspaces don't share the global flow (e.g. a repo with custom slash commands).

## Approach

When at least one pinned command is configured, the attached view grows a single-row **chip row** between the claude PTY pane and the existing attention-line / footer rows. Each chip is `[N] Label` where `N ∈ 1..=9`. The user fires a chip by either:

- **Keyboard:** `Ctrl-x <digit>` after the leader.
- **Mouse:** single-click anywhere inside the chip's rect.

Firing writes the command's text + `\r` to the PTY through the same `session.writer` channel keystrokes use — claude sees it as if the user had typed and submitted it.

Storage matches the existing override pattern used by `branch_prefix` and `custom_instructions`: a global free-text setting plus an optional per-repo column. Per-repo wins entirely when present.

## Decisions

### UX

- **One-row chip strip**, between the claude pane and the attention/footer rows. **Hidden entirely when zero commands are configured** — same layout as today.
- **Chip format**: `[1] PR   [2] FB   [3] /loop /baby…   [4] UR`. Three spaces between chips. Index in dim style, label in default style.
- **Label rules:**
  - `Label=command` → chip shows `Label` verbatim.
  - `command` (no `=`) → chip shows the command text, truncated when its width exceeds **12 columns** to the first 11 columns + `…` (single character, width-1).
  - Both sides of `=` are whitespace-trimmed at parse time. Leading and trailing spaces inside the command body are also trimmed. Internal spacing (e.g. `/loop /babysit-prs`) is preserved verbatim. Pinned commands needing leading/trailing whitespace in the bytes sent to claude are out of scope for v1.
- **Keyboard activation:** after `Ctrl-x` leader, `1`-`9` fire chips at those positions. Out-of-range digits are no-ops (consume the keystroke, clear `leader_pending`).
- **Mouse activation:** the chip row is outside the claude PTY's mouse-capture region; clicks fall through to wsx. A click whose `(col, row)` lands in a chip's rect fires that chip. The existing Shift-drag bypass for text selection is unaffected (drags don't activate chips; only single-click).
- **Send semantics:** `session.scroll_to_live()` then `session.writer.send(format!("{cmd}\r").into_bytes())`. The trailing `\r` matches what `KeyCode::Enter` emits today via `encode_key`, so claude sees it as a real submit.
- **9-command cap:** rendering and key bindings only address positions 1-9. Configured commands past the 9th are silently ignored at render time (`tracing::info!` line for visibility). Configuration itself accepts longer lists without error — keeps editing forgiving.

### Width handling

Chip-row layout fits as many chips as the terminal width allows, **dropping trailing chips that don't fit**, never truncating chip labels further than the 12-char rule above. Rationale: dropping is deterministic (chip 1 always looks the same regardless of width); label-shrinking would make the row visually noisy at narrow widths.

Keyboard shortcuts still work for chips that were dropped from rendering — `Ctrl-x 9` fires the 9th command even if it's not visible. Users learning the feature use the row for discoverability; users who already know their bindings keep working at narrow widths.

### Data model

- **Global:** new setting key `pinned_commands` in the existing `settings` table. Value is a free-text newline-separated string. Editable via the existing config CLI:
  ```bash
  wsx config set pinned_commands @./pinned.txt
  wsx config edit pinned_commands         # opens $EDITOR on the current value
  wsx config get pinned_commands
  wsx config set pinned_commands ""       # clears
  ```

- **Per-repo:** new column on the `repos` table:
  ```sql
  ALTER TABLE repos ADD COLUMN pinned_commands TEXT;
  ```
  Editable via:
  ```bash
  wsx repo set-pinned-commands  <repo> @./repo-pinned.txt
  wsx repo edit-pinned-commands <repo>
  ```
  Also editable in the in-TUI Repo Settings modal (`[s]` on a dashboard row) as a new field alongside `branch_prefix`, `custom_instructions`, `setup_script`, `archive_script`.

- **Override semantics:** if `repos.pinned_commands` is `NULL` or empty after trim, fall back to the global setting. Otherwise the repo value **replaces** the global value (no concatenation). This matches `branch_prefix`'s "per-repo wins entirely" semantics.

- **In-memory representation:**
  ```rust
  pub struct PinnedCommand {
      pub label: String,    // chip text (post-truncation handled at render time)
      pub command: String,  // bytes sent to PTY (sans the \r terminator)
  }
  ```

- **Resolution helper:** `pinned::resolve(global: Option<&str>, repo: Option<&str>) -> Vec<PinnedCommand>`. Called fresh on each `draw()` call, same model as the `nerd_fonts_enabled()` / `pm_enabled()` accessors today. No caching — the call is cheap and lets external `wsx config set` mutations take effect on the next render tick.

### Module layout

```
src/pinned.rs                      NEW
    pub struct PinnedCommand { label: String, command: String }
    pub fn parse(text: &str) -> Vec<PinnedCommand>
    pub fn resolve(global: Option<&str>, repo: Option<&str>) -> Vec<PinnedCommand>

src/ui/attached.rs                 CHANGED
    render() gains a `pinned: &[PinnedCommand]` parameter.
    When non-empty, layout grows from 3 chunks to 4: term / chips / attention / footer.
    New fn render_chip_row(...) returns the rendered Line and the per-chip Rect list
    (so handle_mouse can hit-test).

src/app.rs                         CHANGED
    App gets `pub chip_rects: Vec<Rect>` for the most-recent render's chip positions.
    handle_key_attached: after leader_pending branch, add KeyCode::Char('1'..='9') arm
        that pulls the chip at that index, sends `{cmd}\r` via session.writer.
    handle_mouse: if click coords land in any chip_rect, fire same path.
    draw(): read global + per-repo pinned_commands, call resolve(), pass to attached::render,
        store returned chip_rects for the mouse handler.

src/cli.rs                         CHANGED
    known_setting_key: add "pinned_commands".
    Subcommands: `repo set-pinned-commands <name> <value-or-@file>` and
                 `repo edit-pinned-commands <name>` (mirrors set-setup / edit-setup).

src/store.rs                       CHANGED
    Repo struct gains `pub pinned_commands: Option<String>`.
    SELECT queries and INSERT mappings updated.
    New migration: `ALTER TABLE repos ADD COLUMN pinned_commands TEXT`.

src/ui/modal.rs                    CHANGED
    Repo Settings modal: add `pinned_commands` as a 5th editable field.
```

### Testing strategy

**Unit:**
- `src/pinned.rs#[cfg(test)]`:
  - `parse`: labeled, unlabeled, mixed, lines with `=` inside the command, leading/trailing whitespace, blank lines, comment-style `# ...` lines (decision: comments NOT supported in v1 — `#`-leading lines are treated as commands).
  - `parse`: lines past index 9 are returned (parser doesn't cap) — capping is a render-layer concern.
  - `resolve`: global-only, repo-only, both (repo wins), empty repo string falls back to global, both empty returns empty.
  - Label-truncation rule: ≤12 chars passthrough, >12 chars → first 11 + `…`.

**Integration:**
- `tests/pinned_send.rs`: spawn an attached session against `WSX_CLAUDE_BIN=cat`, simulate `Ctrl-x 1` via `handle_key_attached`, assert the bytes `/pull-request\r` arrived on the writer side. Repeat for `Ctrl-x 9` past list end (no-op).
- Same fixture: simulate a mouse click at coordinates inside `chip_rects[0]`; assert same bytes flow.

**Rendering:**
- `src/ui/attached.rs#[cfg(test)]`:
  - Chip row absent when `pinned.is_empty()` — full term area expanded by one row.
  - Chip row visible with one chip at minimum width.
  - At narrow width, trailing chips drop (assert visible chip count matches the fitted set).
  - Index column is dim-styled; label is default-styled.

**Override semantics:**
- `src/pinned.rs#[cfg(test)]` or integration test: set global `pinned_commands=A`, repo `pinned_commands=B`, assert resolved list is the parse of B (not A+B).

### Notable non-decisions / out of scope for v1

- **Per-workspace overrides.** Repo + global is enough scope. Per-workspace adds storage churn (workspaces are created/destroyed quickly) and no clear use case has been raised.
- **Multi-line / multi-step commands.** Pinned commands are single-line strings. A future extension could detect `\n` and either send line-by-line with delays or use bracketed-paste, but it complicates send semantics for a use case we haven't seen.
- **A picker modal for chip 10+.** Cap is 9. If demand emerges, `Ctrl-x N` (capital) could open a numbered picker for the full list.
- **Variable substitution in commands** (e.g. `{workspace}`, `{branch}`). Out of scope — slash commands targeting claude don't typically need these. Easy to add later if needed.
- **Chips on dashboard / PM pane.** Attached workspace view only. PM pane is documented as inspection-only; dashboard has no claude session to target.
- **Toggle setting to hide the chip row even when commands are configured.** Hiding follows directly from setting `pinned_commands` to empty — no extra flag needed.

## Scope

### In

1. New module `src/pinned.rs`: types + parser + resolve helper.
2. New SQLite column `repos.pinned_commands` with migration.
3. New CLI surface: `pinned_commands` config key, `repo set-pinned-commands`, `repo edit-pinned-commands`.
4. Repo Settings modal: `pinned_commands` editable field.
5. Attached view layout: optional chip row + keyboard + mouse handlers.
6. README updates: chip row + key/click activation, config keys, repo subcommands.
7. Tests for parse, resolve, send, render, override semantics.

### Out

- Per-workspace overrides.
- Multi-line / multi-step commands.
- Picker modal for chips beyond 9.
- Variable substitution.
- Chips outside the attached view.
- A separate "disable chips" toggle (use empty `pinned_commands` instead).

## Acceptance criteria

- With `pinned_commands` empty (global and repo), the attached view renders identically to today — same number of rows.
- With one or more pinned commands configured, a chip row appears between the claude pane and the attention/footer rows.
- `Ctrl-x 1` … `Ctrl-x 9` fire the corresponding chip when it exists; out-of-range digits are no-ops.
- Mouse click inside a chip's rect fires that chip.
- Editing `pinned_commands` via `wsx config set` from another shell is reflected on the next render tick (no restart).
- Per-repo `pinned_commands` value, when non-empty, fully replaces the global list within that repo's workspaces.
- Full test suite passes; `cargo fmt` and `cargo clippy --all-targets -- -D warnings` clean.
