# Chronology Detail Modal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a selected chronology change's full diff in a large, scrollable modal overlay; reduce the docked bar to a list-only navigator that opens the modal.

**Architecture:** Additive first — re-extract the full change on demand from the session log (`load_full_change`), a pure full-diff formatter (`change_detail_lines`), and a `Modal::ChangeDetail` overlay. Then one cohesive task flips the bar to list-only (simplified `nav`, `entry_lines`, render, state) and wires `Enter`/click to open the modal.

**Tech Stack:** Rust, `ratatui`/`crossterm`, `serde_json`. `#[cfg(test)]` unit tests via `cargo test`.

**Builds on (verified current code):**
- `chronology.rs`: `pub struct ChangeEvent { timestamp_ms, tool, file_path, summary, detail }`; `pub fn extract_change_events(v: &serde_json::Value) -> Vec<ChangeEvent>` (clips old/new/content via `clip(s)` = `s.chars().take(DETAIL_MAX_CHARS=600)`); `parse_file(path)` parses each non-empty line via `serde_json` then `out.extend(extract_change_events(&v))`; `resolve_line_in_file(path, detail) -> u32`.
- `chronology_nav.rs`: `ChronoSel{Entry,Detail}`, `NavKey{Up,Down,Top,Bottom,Enter,Esc}`, `NavAction{None,Expand,Collapse,Open,Exit}`, `nav(sel, key, expanded, len)`, `adjust_scroll(scroll, sel_index, visible, len)`.
- `chronology_bar.rs`: `entry_lines(ev, worktree, expanded, width, base_line, highlight) -> Vec<Line>` (header + peek w/ gutter); `EntryHighlight{None,Header,Detail}`.
- `attached.rs`: `render_chronology_bar` loop computes `expanded`, `highlight`, `base_line`, calls `entry_lines`, records entry rects + `chronology_detail_rect`; `ChronologyHits{entries, detail, visible_entries}`; `ChronologyDraw{config,events,worktree,scroll,expanded,focused,sel}`.
- `app.rs`: `chronology_focused: bool`, `chronology_sel: ChronoSel`, `chronology_expanded: Option<usize>`, `chronology_detail_rect`, `chronology_scroll`, `chronology_visible_entries`, `chronology_entry_rects`, `chronology_bar_rect`, `chronology_last_workspace`; `reset_chronology_state_on_workspace_change(...)`.
- `input.rs`: chronology key block maps keys→`NavKey`, runs `nav`, applies `NavAction` (Expand/Collapse set `chronology_expanded`; Open → `open_focused_change` opens editor); mouse detail-click → `open_focused_change`, entry-click → select+expand. `open_focused_change(app, idx)` opens the editor via `editor_open_decision`/`open_in_editor_at`.
- `modal.rs`: `pub enum Modal { … Error{message}, … }`. `render.rs`: modal dispatch `if let Some(m) = &app.modal { match m { … } }` after the view match. `input.rs`: modal keys routed when `app.modal.is_some()`; `Modal::Error` dismissed on Esc/Enter.

---

## File Structure

- `src/activity/chronology.rs` — `ChangeSource`, `extract_change_events(detail_max)`, `parse_file` source population, `load_full_change`.
- `src/ui/chronology_bar.rs` — `change_detail_lines`; later `entry_lines` reduced; `EntryHighlight` removed.
- `src/ui/chronology_nav.rs` — later: single-level `nav`/`NavAction`, `ChronoSel`→index.
- `src/ui/modal.rs` — `Modal::ChangeDetail`.
- `src/app/render.rs` — modal render; later bar wiring.
- `src/app/input.rs` — modal scroll/`e`/`Esc`/wheel; later bar open→modal.
- `src/app.rs` — later state changes.
- `README.md`.

---

## Task 1: Full-change re-extraction (data layer)

**Files:** Modify `src/activity/chronology.rs` (+ fix `ChangeEvent {}` literals crate-wide).

- [ ] **Step 1: Write the failing tests**

Append to `chronology.rs`:

```rust
#[cfg(test)]
mod source_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extract_assigns_index_in_line_and_respects_detail_max() {
        let v: serde_json::Value = serde_json::from_str(r#"{"type":"assistant","timestamp":"2026-05-14T17:00:00.000Z","message":{"content":[{"type":"tool_use","name":"MultiEdit","input":{"file_path":"/wt/a.rs","edits":[{"old_string":"aaaa","new_string":"bbbb"},{"old_string":"cccc","new_string":"dddd"}]}}]}}"#).unwrap();
        let clipped = extract_change_events(&v, 2);
        assert_eq!(clipped.len(), 2);
        assert_eq!(clipped[0].source.index_in_line, 0);
        assert_eq!(clipped[1].source.index_in_line, 1);
        if let ChangeDetail::Edit { new, .. } = &clipped[0].detail {
            assert_eq!(new, "bb", "detail_max=2 clips new_string");
        } else {
            panic!("expected Edit");
        }
        let full = extract_change_events(&v, usize::MAX);
        if let ChangeDetail::Edit { new, .. } = &full[1].detail {
            assert_eq!(new, "dddd", "usize::MAX keeps full text");
        } else {
            panic!("expected Edit");
        }
    }

    #[test]
    fn load_full_change_round_trips_uncliped() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{{}}").unwrap(); // line 0: noise
        writeln!(f, r#"{{"type":"assistant","timestamp":"2026-05-14T17:00:00.000Z","message":{{"content":[{{"type":"tool_use","name":"Edit","input":{{"file_path":"/wt/a.rs","old_string":"OLD","new_string":"A_VERY_LONG_NEW_STRING_BEYOND_ANY_CLIP"}}}}]}}}}"#).unwrap();
        let ev = ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from("/wt/a.rs"),
            summary: String::new(),
            detail: ChangeDetail::Edit { old: "OLD".into(), new: "A_VERY".into() },
            source: ChangeSource { session_file: path.clone(), line_index: 1, index_in_line: 0 },
        };
        let full = load_full_change(&ev).expect("re-extract");
        if let ChangeDetail::Edit { new, .. } = full {
            assert_eq!(new, "A_VERY_LONG_NEW_STRING_BEYOND_ANY_CLIP");
        } else {
            panic!("expected Edit");
        }
    }

    #[test]
    fn load_full_change_none_when_source_empty() {
        let ev = ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Write,
            file_path: PathBuf::from("/wt/a.rs"),
            summary: String::new(),
            detail: ChangeDetail::Write { head: "x".into() },
            source: ChangeSource::default(),
        };
        assert!(load_full_change(&ev).is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib source_tests`
Expected: FAIL — `ChangeSource` / arity of `extract_change_events` / `load_full_change` not defined.

- [ ] **Step 3: Implement**

In `chronology.rs`:

1. Add the source type and field:
```rust
/// Where a `ChangeEvent` was extracted from, so the full (un-clipped) change
/// can be re-read on demand for the detail modal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSource {
    pub session_file: PathBuf,
    pub line_index: usize,
    pub index_in_line: usize,
}
```
Add `pub source: ChangeSource,` to `struct ChangeEvent`.

2. Change `clip` to take a max and thread `detail_max` through `extract_change_events`:
```rust
fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub fn extract_change_events(v: &serde_json::Value, detail_max: usize) -> Vec<ChangeEvent> {
```
Inside, replace each `clip(x)` with `clip(x, detail_max)`. For EVERY `ChangeEvent { … }` literal built here, add:
```rust
                    source: ChangeSource { session_file: PathBuf::new(), line_index: 0, index_in_line: out.len() },
```
(`out.len()` is the event's position in this line's output — assign it in the literal BEFORE the `out.push(...)`. Since the literal is the argument to `push`, `out.len()` is evaluated before the push, giving 0,1,2… in order.)

3. `parse_file` passes the clip and fills the source:
```rust
pub fn parse_file(path: &Path) -> Vec<ChangeEvent> {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (line_index, line) in BufReader::new(file).lines().map_while(|l| l.ok()).enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            for mut ev in extract_change_events(&v, DETAIL_MAX_CHARS) {
                ev.source.session_file = path.to_path_buf();
                ev.source.line_index = line_index;
                out.push(ev);
            }
        }
    }
    out
}
```

4. Add `load_full_change`:
```rust
/// Re-read the un-clipped change for `ev` from its session log. Returns `None`
/// when the source is empty/unreadable or the line/event is gone — callers fall
/// back to the event's clipped `detail`.
pub fn load_full_change(ev: &ChangeEvent) -> Option<ChangeDetail> {
    use std::io::{BufRead, BufReader};
    if ev.source.session_file.as_os_str().is_empty() {
        return None;
    }
    let file = std::fs::File::open(&ev.source.session_file).ok()?;
    let line = BufReader::new(file)
        .lines()
        .map_while(|l| l.ok())
        .nth(ev.source.line_index)?;
    let v: serde_json::Value = serde_json::from_str(&line).ok()?;
    let evs = extract_change_events(&v, usize::MAX);
    evs.into_iter().nth(ev.source.index_in_line).map(|e| e.detail)
}
```

5. Update existing callers/literals so the crate compiles:
   - In `chronology.rs`, the existing `extract_tests` call `extract_change_events(&v)` — change to `extract_change_events(&v, DETAIL_MAX_CHARS)` (or a small value where they assert clipping; they currently assert full short strings, so `DETAIL_MAX_CHARS` keeps them green). Any `ChangeEvent { … }` literal in `chronology.rs` tests gets `source: ChangeSource::default()`.
   - `grep -rn "ChangeEvent {" src` and add `source: ChangeSource::default(),` to EVERY literal outside `extract_change_events` (e.g. `chronology_bar.rs` test `ev()` helper + its gutter tests; any `attached.rs`/`app.rs`/render tests). Import path: `crate::activity::chronology::ChangeSource` where needed.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib source_tests` (3 pass), then `cargo test --lib` (full suite green), `cargo build` (zero warnings), `cargo fmt` + `cargo fmt --check`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(chronology): re-extract full change on demand (ChangeSource + load_full_change)"
```

---

## Task 2: `change_detail_lines` full-diff formatter (pure)

**Files:** Modify `src/ui/chronology_bar.rs`.

- [ ] **Step 1: Write the failing tests**

Add to `chronology_bar.rs` tests:

```rust
    #[test]
    fn change_detail_lines_edit_full_no_cap() {
        let detail = ChangeDetail::Edit {
            old: "o1\no2\no3".into(),
            new: "n1\nn2\nn3".into(),
        };
        let lines = change_detail_lines(&detail, 10);
        assert_eq!(lines.len(), 6, "all 3 old + 3 new, no take(2) cap");
        assert!(lines[0].starts_with("     - o1"));
        assert_eq!(lines[3], "  10 + n1");
        assert_eq!(lines[5], "  12 + n3");
    }

    #[test]
    fn change_detail_lines_write_numbers_all() {
        let detail = ChangeDetail::Write { head: "a\nb".into() };
        let lines = change_detail_lines(&detail, 1);
        assert_eq!(lines, vec!["   1 + a".to_string(), "   2 + b".to_string()]);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib change_detail_lines`
Expected: FAIL — function not defined.

- [ ] **Step 3: Implement**

Add to `chronology_bar.rs` (non-test):

```rust
/// Full change as gutter-formatted display strings (no line cap — the modal
/// scrolls). Removed (`-`) lines get a blank gutter; added (`+`) lines are
/// numbered from `base_line`.
pub fn change_detail_lines(detail: &ChangeDetail, base_line: u32) -> Vec<String> {
    let mut out = Vec::new();
    match detail {
        ChangeDetail::Edit { old, new } => {
            for l in old.lines() {
                out.push(format!("     - {l}"));
            }
            for (k, l) in new.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                out.push(format!("{n:>4} + {l}"));
            }
        }
        ChangeDetail::Write { head } => {
            for (k, l) in head.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                out.push(format!("{n:>4} + {l}"));
            }
        }
        ChangeDetail::None => {}
    }
    out
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib change_detail_lines` (2 pass), `cargo build`, `cargo fmt --check`.

- [ ] **Step 5: Commit**

```bash
git add src/ui/chronology_bar.rs
git commit -m "feat(chronology): change_detail_lines full-diff formatter for the modal"
```

---

## Task 3: `Modal::ChangeDetail` — variant, scroll clamp, render, modal input

**Files:** Modify `src/ui/modal.rs`, `src/app/render.rs`, `src/app/input.rs`. Add `clamp_scroll` to `src/ui/chronology_nav.rs`.

The variant compiles unused this task (the bar wiring that opens it is Task 4). That's intentional.

- [ ] **Step 1: Write the failing test (clamp_scroll)**

Add to `chronology_nav.rs` tests:

```rust
    #[test]
    fn clamp_scroll_bounds() {
        // 100 lines, 20-row body → max top is 80
        assert_eq!(clamp_scroll(85, 100, 20), 80);
        assert_eq!(clamp_scroll(5, 100, 20), 5);
        // content shorter than body → no scroll
        assert_eq!(clamp_scroll(7, 10, 20), 0);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib clamp_scroll`
Expected: FAIL — not defined.

- [ ] **Step 3: Implement `clamp_scroll`**

Add to `chronology_nav.rs` (non-test):

```rust
/// Clamp a scroll offset so a `body`-row viewport never scrolls past the end of
/// `len` lines. Returns 0 when everything fits.
pub fn clamp_scroll(scroll: usize, len: usize, body: usize) -> usize {
    let max = len.saturating_sub(body);
    scroll.min(max)
}
```

- [ ] **Step 4: Add the `Modal::ChangeDetail` variant**

In `src/ui/modal.rs`, add to `enum Modal`:

```rust
    /// Full diff of a chronology change, scrollable.
    ChangeDetail {
        title: String,
        lines: Vec<String>,
        scroll: usize,
        worktree: std::path::PathBuf,
        file: std::path::PathBuf,
        line: u32,
    },
```

If `modal.rs` has helper match arms (e.g. a function classifying modals or rendering a generic box), add a `Modal::ChangeDetail { .. }` arm consistent with the others (it renders via the dedicated function below, so a generic arm can return a placeholder title like `("change", title.clone())` if such a helper exists — match the file's pattern).

- [ ] **Step 5: Render the modal**

In `src/app/render.rs`, in the modal dispatch `match m { … }` (after the view match), add:

```rust
            crate::ui::modal::Modal::ChangeDetail { title, lines, scroll, .. } => {
                render_change_detail_modal(f, area, title, lines, *scroll, &app.theme);
            }
```

And add the renderer (near the other modal render helpers in `render.rs`):

```rust
fn render_change_detail_modal(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    title: &str,
    lines: &[String],
    scroll: usize,
    theme: &crate::ui::theme::Theme,
) {
    use ratatui::layout::Rect;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};
    // Centered box at ~90% of the screen.
    let w = area.width.saturating_mul(9) / 10;
    let h = area.height.saturating_mul(9) / 10;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let modal = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(ratatui::style::Style::default().fg(theme.path));
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    // Body: reserve the last row for a footer hint.
    let body_h = inner.height.saturating_sub(1) as usize;
    let scroll = crate::ui::chronology_nav::clamp_scroll(scroll, lines.len(), body_h);
    let visible: Vec<Line> = lines
        .iter()
        .skip(scroll)
        .take(body_h)
        .map(|l| {
            let clipped: String = l.chars().take(inner.width as usize).collect();
            Line::from(Span::raw(clipped))
        })
        .collect();
    let body_area = Rect { height: inner.height.saturating_sub(1), ..inner };
    f.render_widget(Paragraph::new(visible), body_area);
    let end = (scroll + body_h).min(lines.len());
    let footer = format!(
        "↑/↓ j/k  PgUp/PgDn  g/G  ·  e editor  ·  Esc close    {}-{}/{}",
        scroll + 1,
        end,
        lines.len()
    );
    let footer_area = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer.chars().take(inner.width as usize).collect::<String>(),
            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::DIM),
        ))),
        footer_area,
    );
}
```
(Adapt `theme.path`/`theme.header_style()` to the file's existing modal styling conventions; match how other modal helpers obtain colors.)

- [ ] **Step 6: Modal input handling**

In `src/app/input.rs`, find the modal key handler (where `Modal::Error`/`Modal::UpdatesPanel` etc. are matched while `app.modal.is_some()`). Add a `Modal::ChangeDetail` arm. Because the variant fields are owned, mutate via a `match &mut app.modal`:

```rust
        Some(crate::ui::modal::Modal::ChangeDetail { lines, scroll, worktree, file, line, .. }) => {
            const PAGE: usize = 10;
            let len = lines.len();
            match k.code {
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1).min(len.saturating_sub(1)),
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => *scroll = scroll.saturating_add(PAGE).min(len.saturating_sub(1)),
                KeyCode::PageUp => *scroll = scroll.saturating_sub(PAGE),
                KeyCode::Char('g') => *scroll = 0,
                KeyCode::Char('G') => *scroll = len.saturating_sub(1),
                KeyCode::Esc => { app.modal = None; }
                KeyCode::Char('e') => {
                    let (worktree, file, line) = (worktree.clone(), file.clone(), *line);
                    open_change_in_editor(app, &worktree, &file, line);
                }
                _ => {}
            }
            return Ok(());
        }
```
(Match the actual structure of the modal handler — it may be a `match app.modal { … }` or a helper `handle_key_modal`. Place this arm consistently. The renderer re-clamps `scroll` against the real body height, so the coarse `len-1` clamp here is safe.)

Add `open_change_in_editor` (reuse the existing editor decision/launch, factored from `open_focused_change`):

```rust
/// Open `file` at `line` using the configured editor, surfacing a Modal::Error
/// when unset or on failure. (Shared by the detail modal's `e`.)
fn open_change_in_editor(app: &mut App, worktree: &Path, file: &Path, line: u32) {
    use crate::commands::external::{EditorOpenDecision, editor_open_decision};
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
            if let Err(e) =
                crate::commands::external::open_in_editor_at(worktree, file, line, Some(&cmd))
            {
                app.modal = Some(crate::ui::modal::Modal::Error {
                    message: format!("Failed to open editor: {e}"),
                });
            }
        }
    }
}
```

- [ ] **Step 7: Mouse wheel + click-outside**

In `handle_mouse`, before the existing chronology-bar wheel block, add: if `matches!(app.modal, Some(Modal::ChangeDetail{..}))` and the event is a wheel, adjust `scroll` (same +1/-1 logic via `match &mut app.modal`) and return; if it's a left-click outside the modal box, `app.modal = None`. (A simple approach: any left-click while `Modal::ChangeDetail` is open closes it — modal is a focused overlay. Keep it minimal: wheel scrolls, click closes.)

- [ ] **Step 8: Verify**

Run: `cargo test --lib clamp_scroll` (pass), `cargo test --lib` (no regressions), `cargo build` (zero warnings — the variant is constructed in Task 4; until then it's only matched, which is fine; if an "unused variant" or unreachable warning appears, it won't because all arms handle it — confirm), `cargo fmt --check`.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(chronology): ChangeDetail modal (variant, render, scroll input, e to open)"
```

---

## Task 4: Bar → list-only + open the modal

**Files:** Modify `src/ui/chronology_nav.rs`, `src/ui/chronology_bar.rs`, `src/ui/attached.rs`, `src/app.rs`, `src/app/render.rs`, `src/app/input.rs`. This is one cohesive task (the pieces reference each other); land it together so the tree compiles.

- [ ] **Step 1: Simplify the nav reducer**

In `chronology_nav.rs`: delete `ChronoSel` (and its `Default`/`index`); `NavAction` becomes `{ None, Open(usize), Exit }` (remove `Expand`/`Collapse`); rewrite `nav` to a single-level index machine:

```rust
pub fn nav(sel: usize, key: NavKey, len: usize) -> (usize, NavAction) {
    if key == NavKey::Esc {
        return (sel, NavAction::Exit);
    }
    if len == 0 {
        return (sel, NavAction::None);
    }
    let last = len - 1;
    match key {
        NavKey::Down => ((sel + 1).min(last), NavAction::None),
        NavKey::Up => (sel.saturating_sub(1), NavAction::None),
        NavKey::Top => (0, NavAction::None),
        NavKey::Bottom => (last, NavAction::None),
        NavKey::Enter => (sel, NavAction::Open(sel)),
        NavKey::Esc => unreachable!(),
    }
}
```
Rewrite the reducer tests in this module accordingly: `Up`/`Down` clamp, `Top`/`Bottom`, `Enter`→`Open(sel)`, `Esc`→`Exit`, `len==0` no-op. Keep `adjust_scroll`/`clamp_scroll` and their tests.

- [ ] **Step 2: Reduce `entry_lines` to header-only**

In `chronology_bar.rs`: remove the `EntryHighlight` enum and the peek/gutter/`base_line`/`expanded` logic. New signature + body:

```rust
/// One bar row: `HH:MM <abbreviated path>`, reversed when `selected`.
pub fn entry_lines(ev: &ChangeEvent, worktree: &Path, width: u16, selected: bool) -> Vec<Line<'static>> {
    let rel = relative_display(&ev.file_path, worktree);
    let path_budget = (width as usize).saturating_sub(6);
    let path = abbreviate_path(&rel, path_budget);
    let base = Style::default();
    let style = if selected { base.add_modifier(Modifier::REVERSED) } else { base };
    let time_style = if selected {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::DIM)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    vec![Line::from(vec![
        Span::styled(hhmm(ev.timestamp_ms), time_style),
        Span::styled(" ", style),
        Span::styled(path, style),
    ])]
}
```
Update/trim the chronology_bar tests that referenced the old peek/highlight/`base_line` (keep `relative_path_*`, `auto_hide_*`, `abbreviate_*`, `change_detail_lines_*`; replace the entry/peek/highlight tests with a couple asserting the single header line and that `selected` reverses it). Keep `change_detail_lines` (Task 2) intact.

- [ ] **Step 3: `render_chronology_bar` list-only**

In `attached.rs`: `ChronologyDraw` drops `expanded` and `sel: ChronoSel`, gains `sel: usize`. `ChronologyHits` drops `detail`; keep `entries`, `visible_entries`. In the loop, remove the `expanded`/`base_line`/`EntryHighlight`/detail-rect logic; compute `let selected = draw.focused && i == draw.sel;` and call `entry_lines(ev, draw.worktree, inner_width, selected)`. Record entry rects + `visible_entries` as before. Update `PanesDrawOutput` to drop `chronology_detail_rect`.

- [ ] **Step 4: App state**

In `app.rs`: remove `chronology_expanded` and `chronology_detail_rect` (and their inits + the `render.rs` clear of `chronology_detail_rect`). Change `chronology_sel: ChronoSel` → `chronology_sel: usize` (init `0`). Update `reset_chronology_state_on_workspace_change` to take/reset `chronology_sel: &mut usize` (`*sel = 0`) and `chronology_focused: &mut bool` (drop the `expanded`/`ChronoSel` params); update its call site in `render.rs`.

- [ ] **Step 5: render.rs bar wiring**

In `render.rs` `View::Attached`: build `ChronologyDraw { …, sel: app.chronology_sel }` (no `expanded`); drop the `app.chronology_detail_rect = …` store and the `chronology_detail_rect` clear; keep `chronology_entry_rects`/`chronology_visible_entries`. Remove the `base_line`/`resolve_line_in_file` call added for the bar peek (the modal computes its own line on open).

- [ ] **Step 6: input.rs — open the modal**

Replace the chronology key block's `nav` usage and `NavAction` handling: map keys→`NavKey`, call `nav(app.chronology_sel, navkey, len)`, store `app.chronology_sel`, and match `NavAction`:
- `None` → nothing; `Exit` → `app.chronology_focused = false`;
- `Open(i)` → `open_change_modal(app, i)`.
Remove `Expand`/`Collapse`/`Detail` handling. In `handle_mouse`, the entry-click branch → `open_change_modal(app, idx)` (and set `chronology_focused = true`, `chronology_sel = idx`); remove the detail-rect click branch. Repurpose/replace `open_focused_change` with:

```rust
/// Open the chronology entry at `idx` in the full-change detail modal.
fn open_change_modal(app: &mut App, idx: usize) {
    let Some((_ws_id, worktree)) = focused_attached_workspace(app) else { return };
    let Some(ev) = focused_attached_workspace(app)
        .and_then(|(ws_id, _)| app.chronology.get(&ws_id))
        .and_then(|t| t.events().get(idx).cloned())
    else { return };
    let detail = crate::activity::chronology::load_full_change(&ev).unwrap_or(ev.detail.clone());
    let line = crate::activity::chronology::resolve_line_in_file(&ev.file_path, &detail);
    let lines = crate::ui::chronology_bar::change_detail_lines(&detail, line);
    let rel = crate::ui::chronology_bar::relative_display(&ev.file_path, &worktree);
    let title = format!("{} {}", crate::ui::chronology_bar::hhmm_pub(ev.timestamp_ms), rel);
    app.modal = Some(crate::ui::modal::Modal::ChangeDetail {
        title,
        lines,
        scroll: 0,
        worktree,
        file: ev.file_path.clone(),
        line,
    });
}
```
NOTE: `hhmm` is private in `chronology_bar.rs`; expose a `pub fn hhmm_pub(ms: i64) -> String` (or make `hhmm` pub) for the title, OR format the time inline. `relative_display` is already `pub`. `Timeline::events()` returns `&[ChangeEvent]`; `.get(idx).cloned()` needs `ChangeEvent: Clone` (it derives Clone). Resolve the borrow by cloning `ev` before mutating `app.modal` (shown above).

- [ ] **Step 7: Verify**

Run: `cargo test --lib` (all pass — nav/entry_lines/clamp_scroll/change_detail_lines/source), `cargo build` (zero warnings), `cargo clippy --lib` (no new lints), `cargo fmt --check` (clean).
Manual: focus the bar (`Ctrl-x`+arrow), move with `j`/`k`, `Enter` → the modal opens with the full diff + line-number gutter; scroll with arrows/`j`/`k`/`PgUp`/`PgDn`/`g`/`G`/wheel; `e` opens the editor at the line; `Esc`/click closes. Click a bar entry → modal opens.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(chronology): list-only bar that opens the full-change detail modal"
```

---

## Task 5: README

**Files:** Modify `README.md`.

- [ ] **Step 1: Update the Change chronology section**

Replace the inline-peek / in-bar-expand / two-step-open prose with: the bar is the time-ordered list; `Enter` (or click) opens the selected change in a **scrollable modal** showing the full diff with a line-number gutter; in the modal, `↑/↓ j/k PgUp/PgDn g/G` and the wheel scroll, `e` opens the file in your editor at the change line, `Esc` (or click outside) closes. Remove now-inaccurate mentions of the inline diff peek / in-bar gutter / expand-collapse. Match the README's prose style.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document the chronology full-change detail modal"
```

---

## Self-Review (completed during planning)

**Spec coverage:** full-change re-extraction (`ChangeSource`/`load_full_change`, T1) ✓; full-diff formatter (`change_detail_lines`, T2) ✓; modal variant + render + scroll (kbd+wheel) + `e` + `Esc` (T3) ✓; bar list-only + simplified nav/state + open-modal wiring (T4) ✓; README (T5) ✓; removals (peek/expand/two-level/gutter-in-bar/detail-rect) enacted in T4 ✓; clamp helper tested (T3) ✓.

**Placeholder scan:** code shown for each step; the few "match the file's pattern / adapt theme" notes point at concrete existing conventions, not deferred work. T1 and T4 explicitly enumerate the cross-file literal/signature fixups required to keep the tree compiling.

**Type consistency:** `ChangeSource{session_file,line_index,index_in_line}`, `extract_change_events(v, detail_max)`, `load_full_change(&ChangeEvent)->Option<ChangeDetail>`, `change_detail_lines(&ChangeDetail, base_line:u32)->Vec<String>`, `clamp_scroll(scroll,len,body)`, `nav(sel:usize,key,len)->(usize,NavAction{None,Open,Exit})`, `entry_lines(ev,worktree,width,selected:bool)`, `ChronologyDraw{…,sel:usize}` (no `expanded`), `ChronologyHits{entries,visible_entries}` (no `detail`), `Modal::ChangeDetail{title,lines,scroll,worktree,file,line}`, `open_change_modal`, `open_change_in_editor` — consistent across tasks.
