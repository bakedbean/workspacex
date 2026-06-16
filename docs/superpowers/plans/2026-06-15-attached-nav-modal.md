# Attached-view Navigation Modal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the always-on keybind footer in the attached (agent chat) view with a Docker-TUI-style modal armed by Ctrl-x, and reclaim the footer rows by dropping the focused workspace + attention items onto a single bottom line.

**Architecture:** The `^x` leader already exists (`app.leader_pending` + dispatch in `handle_key_attached`). The "modal" is a rendered overlay keyed off `leader_pending` plus a new highlight index `app.leader_selected` — letter accelerators keep working unchanged; we only add ↑↓ (move highlight) and Enter (fire highlight). A single source-of-truth item list (`nav_menu.rs`) feeds both the renderer and the Enter dispatch so they can't drift. `layout_chrome` loses the 2-row footer; the freed row hosts `agent-bar + label + attention`. A `^x: menu` hint moves to the left of the chip row.

**Tech Stack:** Rust, ratatui, crossterm, tokio. Tests use `ratatui::backend::TestBackend` and `#[tokio::test]`.

---

## File Structure

- `src/app.rs` — add `leader_selected: usize` field to `App`.
- `src/ui/attached/nav_menu.rs` *(new)* — `NavItem`, `nav_menu_items()`, `pm_nav_menu_items()`, and `render_nav_overlay()`. Single source of truth for the menu.
- `src/ui/attached/mod.rs` — `mod nav_menu;` + re-exports; rework `layout_chrome` (drop footer rows); add `bottom_line_prefix_width()` and `bottom_line()`; rework `render_panes` (compose bottom line, drop footer, carve chip-row menu hint).
- `src/ui/attached/footer.rs` — `footer_line` is no longer rendered as chrome; its key lists are superseded by `nav_menu.rs`. The file/tests are removed in Task 8.
- `src/ui/attached/chip_row.rs` — unchanged (we carve the menu-hint area *before* calling it).
- `src/app/render.rs` — draw the overlay at end of `draw()`; reduce attention `max_width` and offset attention rects by the bottom-line prefix; update both `render_panes`/`layout_chrome` call sites.
- `src/app/input.rs` — extract `dispatch_leader_action()` / `dispatch_pm_leader_action()`; add ↑↓/Enter handling while `leader_pending`; reset `leader_selected` on arm.
- `src/app/input_tests.rs` — new tests for nav + Enter parity.

---

## Task 1: Add `leader_selected` to App

**Files:**
- Modify: `src/app.rs:137` (field), `src/app.rs:317` (init)

- [ ] **Step 1: Add the field**

In `src/app.rs`, immediately after the `leader_pending` field (line 137):

```rust
    pub leader_pending: bool,
    /// Highlighted row in the Ctrl-x navigation overlay. Reset to 0 each time
    /// the attached/PM leader is armed; adjusted by ↑↓ while the overlay is up.
    pub leader_selected: usize,
    pub z_leader_pending: bool,
```

- [ ] **Step 2: Initialise it**

In the `App` constructor, immediately after `leader_pending: false,` (line 317):

```rust
            leader_pending: false,
            leader_selected: 0,
            z_leader_pending: false,
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(attached): add leader_selected highlight index to App"
```

---

## Task 2: Nav menu source of truth

**Files:**
- Create: `src/ui/attached/nav_menu.rs`
- Modify: `src/ui/attached/mod.rs:15-24` (module decl + re-exports)

- [ ] **Step 1: Create `nav_menu.rs` with the item model and lists**

Create `src/ui/attached/nav_menu.rs`:

```rust
//! Single source of truth for the Ctrl-x navigation overlay: the ordered
//! action list (so the renderer and the Enter-dispatch can't drift) plus the
//! overlay renderer itself.

use super::*;
use crate::ui::footer::key_for_glyph;
use ratatui::widgets::{Block, Borders, Clear};

/// One row in the navigation overlay. `glyph` is the key as printed in the
/// pill (`"d"`, `"x"`, `"←→"`); it doubles as the Enter-dispatch key via
/// [`crate::ui::footer::key_for_glyph`]. `label` is the human description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavItem {
    pub glyph: &'static str,
    pub label: &'static str,
}

/// The attached coding-agent view's action list. Multi-pane mode swaps
/// `detach` → `close pane` and adds a `←→ focus pane` row. Order here is the
/// order rows render AND the order ↑↓ walks — it is the contract the Enter
/// handler relies on.
pub fn nav_menu_items(multi_pane: bool) -> Vec<NavItem> {
    let mut items = vec![NavItem {
        glyph: "d",
        label: if multi_pane { "close pane" } else { "detach" },
    }];
    if multi_pane {
        items.push(NavItem { glyph: "←→", label: "focus pane" });
    }
    items.extend([
        NavItem { glyph: "u", label: "updates" },
        NavItem { glyph: "a", label: "agents" },
        NavItem { glyph: "e", label: "edit" },
        NavItem { glyph: "t", label: "open terminal" },
        NavItem { glyph: "v", label: "diff" },
        NavItem { glyph: "g", label: "lazygit" },
        NavItem { glyph: "k", label: "processes" },
        NavItem { glyph: "x", label: "send literal ^x" },
    ]);
    items
}

/// The PM pane's smaller action list.
pub fn pm_nav_menu_items() -> Vec<NavItem> {
    vec![
        NavItem { glyph: "d", label: "detach" },
        NavItem { glyph: "u", label: "updates" },
        NavItem { glyph: "x", label: "send literal ^x" },
    ]
}

/// Resolve a highlighted index to the key press it fires, reusing the footer's
/// glyph→key map (`"←→"` → Right, single chars → that char). `None` when the
/// index is out of range or the glyph has no single-key mapping.
pub fn nav_item_key(items: &[NavItem], selected: usize) -> Option<crossterm::event::KeyEvent> {
    items.get(selected).and_then(|i| key_for_glyph(i.glyph))
}
```

- [ ] **Step 2: Register the module and re-export**

In `src/ui/attached/mod.rs`, add to the `mod` block (after line 17 `mod footer;`):

```rust
mod agents_row;
mod chip_row;
mod footer;
mod nav_menu;
```

And in the re-export block (after line 23-24):

```rust
pub use agents_row::agent_switch_keys;
pub use nav_menu::{NavItem, nav_item_key, nav_menu_items, pm_nav_menu_items, render_nav_overlay};
pub(crate) use chip_row::render_chip_row;
```

(`render_nav_overlay` is added in Task 3; declaring the re-export now is fine because the next task adds it before any build that needs it — but to keep this task green, add `render_nav_overlay` to the re-export only in Task 3. For now re-export just the items:)

```rust
pub use nav_menu::{NavItem, nav_item_key, nav_menu_items, pm_nav_menu_items};
```

- [ ] **Step 3: Write tests at the bottom of `nav_menu.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_lists_detach_not_close_pane() {
        let items = nav_menu_items(false);
        assert_eq!(items[0], NavItem { glyph: "d", label: "detach" });
        assert!(!items.iter().any(|i| i.label == "focus pane"));
        // Stable tail order the Enter handler depends on.
        assert_eq!(items.last().unwrap().glyph, "x");
    }

    #[test]
    fn multi_pane_swaps_close_pane_and_adds_focus() {
        let items = nav_menu_items(true);
        assert_eq!(items[0], NavItem { glyph: "d", label: "close pane" });
        assert_eq!(items[1], NavItem { glyph: "←→", label: "focus pane" });
        assert!(items.len() == nav_menu_items(false).len() + 1);
    }

    #[test]
    fn nav_item_key_maps_glyph_to_keypress() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let items = nav_menu_items(true);
        let updates = items.iter().position(|i| i.glyph == "u").unwrap();
        assert_eq!(
            nav_item_key(&items, updates),
            Some(crossterm::event::KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE))
        );
        let focus = items.iter().position(|i| i.glyph == "←→").unwrap();
        // "←→" collapses to Right (forward) per key_for_glyph.
        assert_eq!(
            nav_item_key(&items, focus),
            Some(crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        );
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p wsx nav_menu`
Expected: 3 tests pass. (Crate name may differ; use `cargo test nav_menu::` if `-p wsx` is rejected.)

- [ ] **Step 5: Commit**

```bash
git add src/ui/attached/nav_menu.rs src/ui/attached/mod.rs
git commit -m "feat(attached): nav-menu item lists as single source of truth"
```

---

## Task 3: Input — leader dispatch extraction + menu navigation (attached view)

**Files:**
- Modify: `src/app/input.rs` (extract `dispatch_leader_action`; rework the `leader_pending` block in `handle_key_attached`)
- Test: `src/app/input_tests.rs`

- [ ] **Step 1: Extract the action dispatch into a free function**

Move the body of the current `if app.leader_pending { app.leader_pending = false; match k.code { ... } }` block in `handle_key_attached` (lines ~742-901) into a new free function. Add near the other attached helpers in `src/app/input.rs`:

```rust
/// Fire a single attached-view leader action for `k` (already-armed leader).
/// Extracted so both the letter-accelerator path and the overlay's Enter path
/// dispatch through identical code. Caller clears `leader_pending` first.
async fn dispatch_leader_action(
    app: &mut App,
    target: crate::ui::split::AttachTarget,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let id = target.workspace_id;
    let session = match app.sessions.get(target.instance) {
        Some(s) => s,
        None => {
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    match k.code {
        // ... PASTE the existing match arms verbatim from the old block:
        // KeyCode::Char('d') => { ...close/detach... }
        // KeyCode::Esc => { ...save layout + detach... }
        // KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => { ...focus_direction... }
        // KeyCode::Char('x') => { ...send 0x18... }
        // KeyCode::Char('u') | 'a' | 'e' | 't' | 'v' | 'g' | 'c' | 'k' => { ... }
        // KeyCode::Char(c @ '1'..='9') => { ...fire pinned... }
        // KeyCode::Char(c) => { ...agent switch fallback... }
        _ => return Ok(()),
    }
}
```

Copy every existing arm unchanged (they already use `id`, `session`, and `app.view`). The arms `return Ok(())` as before.

- [ ] **Step 2: Replace the `leader_pending` block in `handle_key_attached`**

Where the old block was (lines ~740-902), put:

```rust
    // Leader armed: ↑↓ move the overlay highlight (leader stays armed); Enter
    // fires the highlighted action; any other key is a direct accelerator that
    // fires immediately. Esc / second Ctrl-x fall through to the dispatch which
    // clears the leader.
    if app.leader_pending {
        let multi_pane = matches!(&app.view, View::Attached(s) if s.leaf_count() > 1);
        let items = crate::ui::attached::nav_menu_items(multi_pane);
        match k.code {
            KeyCode::Up => {
                let n = items.len();
                app.leader_selected = (app.leader_selected + n - 1) % n;
                return Ok(());
            }
            KeyCode::Down => {
                let n = items.len();
                app.leader_selected = (app.leader_selected + 1) % n;
                return Ok(());
            }
            KeyCode::Enter => {
                app.leader_pending = false;
                if let Some(key) =
                    crate::ui::attached::nav_item_key(&items, app.leader_selected)
                {
                    return dispatch_leader_action(app, target, key).await;
                }
                return Ok(());
            }
            _ => {
                app.leader_pending = false;
                return dispatch_leader_action(app, target, k).await;
            }
        }
    }
```

- [ ] **Step 3: Reset the highlight when the leader is armed**

In `handle_key_attached`, the arm line (was line ~903-905):

```rust
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        app.leader_selected = 0;
        return Ok(());
    }
```

- [ ] **Step 4: Write the navigation + Enter-parity test**

Add to `src/app/input_tests.rs` inside the same module that holds `ctrl_x_arrow_moves_focus_in_split` (reuse its setup pattern). This single-pane test needs only one ready workspace + session:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_down_enter_fires_highlighted_action() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "a",
                branch: "repo/a",
                worktree_path: &std::path::PathBuf::from("/tmp/wsx-nav-a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store.set_workspace_state(id, WorkspaceState::Ready).unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let inst = test_primary_instance(&app, id);
        app.sessions
            .spawn(
                inst,
                id,
                std::path::Path::new("."),
                80,
                24,
                crate::pty::session::SpawnMode::Fresh {
                    rename_ctx: None,
                    custom_instructions: None,
                    doctrine: None,
                    additional_dirs: vec![],
                    yolo: false,
                },
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
            .unwrap();
        let target = test_target(&app, id);
        app.view = crate::ui::View::Attached(AttachedState::single(target));

        // Arm leader (selected=0 => "detach"), Down once => index 1 ("updates").
        handle_key_attached(&mut app, target, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)).await.unwrap();
        assert!(app.leader_pending);
        assert_eq!(app.leader_selected, 0);
        handle_key_attached(&mut app, target, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).await.unwrap();
        assert_eq!(app.leader_selected, 1);
        assert!(app.leader_pending, "↑↓ keep the leader armed");

        // Enter fires "updates" — same effect as pressing 'u' after ^x.
        handle_key_attached(&mut app, target, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).await.unwrap();
        assert!(!app.leader_pending);
        assert!(
            matches!(app.modal, Some(crate::ui::modal::Modal::UpdatesPanel { .. })),
            "Enter on the updates row opens the updates panel"
        );
    }
```

- [ ] **Step 5: Run the test**

Run: `cargo test ctrl_x_down_enter_fires_highlighted_action -- --nocapture`
Expected: PASS. Also run `cargo test ctrl_x_arrow_moves_focus_in_split` — still PASS (Left still focuses).

- [ ] **Step 6: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(attached): leader menu nav (up/down/enter) + dispatch extraction"
```

---

## Task 4: Render the navigation overlay

**Files:**
- Modify: `src/ui/attached/nav_menu.rs` (add `render_nav_overlay`)
- Modify: `src/ui/attached/mod.rs` (re-export `render_nav_overlay`)

- [ ] **Step 1: Add the overlay renderer to `nav_menu.rs`**

Append before the `#[cfg(test)]` block:

```rust
/// Draw the centered Ctrl-x navigation overlay. Keybind column first, then
/// label; the `selected` row carries a `▌` marker. `pinned_hint` adds a
/// "1-9 pinned" note to the footer hint line when the workspace has pinned
/// commands. Mirrors the dim bordered framing of the other modals.
pub fn render_nav_overlay(
    f: &mut Frame,
    area: Rect,
    items: &[NavItem],
    selected: usize,
    pinned_hint: bool,
    theme: &Theme,
) {
    let hint = if pinned_hint {
        "↑↓ move · enter · 1-9 pinned · esc"
    } else {
        "↑↓ move · enter · esc"
    };
    // Width: marker(2) + pill(" g " = 3) + gap(3) + label, max over rows & hint.
    let row_w = |label_len: usize| 2 + 3 + 3 + label_len;
    let content_w = items
        .iter()
        .map(|i| row_w(i.label.chars().count()))
        .chain(std::iter::once(hint.chars().count()))
        .max()
        .unwrap_or(20);
    let w = (content_w as u16 + 4).min(area.width); // + borders + 1 pad each side
    let h = (items.len() as u16 + 4).min(area.height); // borders + blank + hint

    let rect = centered_box(area, w, h);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("actions")
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(items.len() + 2);
    for (i, item) in items.iter().enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(4);
        if i == selected {
            spans.push(Span::styled("▌ ".to_string(), Style::default().fg(theme.waiting)));
        } else {
            spans.push(Span::raw("  ".to_string()));
        }
        spans.extend(super::key_pill_spans(item.glyph, theme));
        spans.push(Span::styled(
            format!("   {}", item.label),
            Style::default().fg(theme.path),
        ));
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(Vec::<Span<'static>>::new()));
    lines.push(Line::from(Span::styled(
        hint.to_string(),
        theme.dim_style(),
    )));
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

/// Center a `w`×`h` rect inside `area` (local copy so nav_menu owns its
/// framing without reaching into `ui::modal`).
fn centered_box(area: Rect, w: u16, h: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(h), Constraint::Min(0)])
        .split(area)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(w), Constraint::Min(0)])
        .split(v)[1]
}
```

(`Frame`, `Rect`, `Line`, `Span`, `Style`, `Paragraph`, `Text`, `Layout`, `Constraint`, `Direction`, `Theme` all come in via `use super::*;` at the top of `nav_menu.rs`. If `Text` is not in scope, add `use ratatui::text::Text;`.)

- [ ] **Step 2: Re-export it**

In `src/ui/attached/mod.rs`, extend the nav_menu re-export:

```rust
pub use nav_menu::{NavItem, nav_item_key, nav_menu_items, pm_nav_menu_items, render_nav_overlay};
```

- [ ] **Step 3: Render test**

Add to the `tests` module in `nav_menu.rs`:

```rust
    #[test]
    fn overlay_renders_keybind_then_label_with_marker() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = Theme::wsx();
        let items = nav_menu_items(false);
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| {
            render_nav_overlay(f, Rect::new(0, 0, 60, 20), &items, 1, true, &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let text: String = (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("actions"), "titled actions:\n{text}");
        assert!(text.contains("detach"), "lists detach");
        assert!(text.contains("send literal ^x"), "lists send-^x");
        // Selected row (index 1 = updates) carries the ▌ marker.
        assert!(text.contains("▌"), "highlight marker present");
        assert!(text.contains("1-9 pinned"), "pinned hint shown");
    }
```

- [ ] **Step 4: Run**

Run: `cargo test overlay_renders_keybind_then_label_with_marker`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/attached/nav_menu.rs src/ui/attached/mod.rs
git commit -m "feat(attached): centered Ctrl-x navigation overlay renderer"
```

---

## Task 5: Draw the overlay from `draw()`

**Files:**
- Modify: `src/app/render.rs` (end of `draw()`, ~line 764)

- [ ] **Step 1: Add a helper + call it at the end of `draw()`**

In `src/app/render.rs`, just before the closing brace of `draw()` (after the usage-picker block, ~line 764), add:

```rust
    draw_attached_nav_overlay(f, area, app);
}

/// Render the Ctrl-x navigation overlay when the leader is armed in an
/// attached view. Keyed off `leader_pending`, so letter accelerators and the
/// overlay share one state. Context (multi-pane vs PM) selects the item list.
fn draw_attached_nav_overlay(f: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    if !app.leader_pending {
        return;
    }
    let (items, pinned_hint) = match &app.view {
        crate::ui::View::Attached(state) => (
            crate::ui::attached::nav_menu_items(state.leaf_count() > 1),
            !app.pinned_commands_cache.is_empty(),
        ),
        crate::ui::View::AttachedPm => (crate::ui::attached::pm_nav_menu_items(), false),
        _ => return,
    };
    crate::ui::attached::render_nav_overlay(
        f,
        area,
        &items,
        app.leader_selected,
        pinned_hint,
        &app.theme,
    );
}
```

Remove the now-duplicated closing brace of `draw()` (the helper above introduces the `}` that closes `draw()` then opens the new fn).

- [ ] **Step 2: Build + smoke test**

Run: `cargo build`
Expected: compiles. Run `cargo test --lib` — existing tests still pass (footer still present at this point; overlay now also draws when armed).

- [ ] **Step 3: Commit**

```bash
git add src/app/render.rs
git commit -m "feat(attached): draw Ctrl-x nav overlay when leader armed"
```

---

## Task 6: Reclaim footer rows + compose the bottom line

**Files:**
- Modify: `src/ui/attached/mod.rs` (`layout_chrome`, `render_panes`, add `bottom_line` helpers)
- Modify: `src/app/render.rs` (both call sites; attention width + rect offset)

- [ ] **Step 1: Add bottom-line helpers to `mod.rs`**

Add to `src/ui/attached/mod.rs` (near `title_bar_spans`):

```rust
/// Width in columns of the bottom line's leading `[agent-bar ]label   ` prefix,
/// before the attention items begin. Shared by `render.rs` (to shrink the
/// attention width budget and offset its click rects) and `bottom_line` (to
/// draw it) so the two never disagree.
pub fn bottom_line_prefix_width(label: &str, agent: Option<AgentKind>) -> u16 {
    let bar = if agent.is_some() { 2 } else { 0 }; // "▎" + " "
    bar + label.chars().count() as u16 + 3 // 3-col gap before attention
}

/// Build the bottom line: optional agent identity bar, the focused workspace
/// label (header style), then — when present — the attention items. The
/// attention `Line` is pre-truncated by the caller to the post-prefix width.
fn bottom_line(
    label: &str,
    agent: Option<AgentKind>,
    attention: Option<Line<'static>>,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    if let Some(a) = agent {
        spans.push(Span::styled("▎".to_string(), theme.agent_style(a)));
        spans.push(Span::raw(" ".to_string()));
    }
    spans.push(Span::styled(label.to_string(), theme.header_style()));
    if let Some(line) = attention {
        spans.push(Span::raw("   ".to_string()));
        spans.extend(line.spans);
    }
    Line::from(spans)
}
```

- [ ] **Step 2: Rework `layout_chrome` to drop the footer**

Replace `layout_chrome` (lines ~243-262). The footer rows are gone; the bottom line is always 1 tall:

```rust
/// Carve the attached view's `area` into pane / chip / bottom-line / agents
/// sub-areas. The chip row and bottom line are each 1 cell tall and always
/// present; the agents row is 1 cell when `agents_present`, else 0. The
/// bottom line hosts the focused workspace label + attention items (no
/// separate footer — navigation lives in the Ctrl-x overlay).
pub fn layout_chrome(area: Rect, agents_present: bool) -> (Rect, Rect, Rect, Rect) {
    let agents_h = if agents_present { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),        // chip row
            Constraint::Length(1),        // bottom line (label + attention)
            Constraint::Length(agents_h), // agents row (0 when absent)
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2], chunks[3])
}
```

Update the `layout_chrome` test (lines ~353-391) to the new 4-tuple/2-arg shape:

```rust
    #[test]
    fn layout_chrome_reclaims_footer_rows() {
        let area = ratatui::layout::Rect::new(0, 0, 80, 30);
        let (pane, chip, bottom, agents) = layout_chrome(area, false);
        assert_eq!(chip.height, 1);
        assert_eq!(bottom.height, 1, "bottom line always present");
        assert_eq!(agents.height, 0);
        assert_eq!(
            pane.height + chip.height + bottom.height + agents.height,
            area.height
        );
        // Pane reclaims the old 2-row footer + attention row vs the old layout.
        let (_, _, _, agents2) = layout_chrome(area, true);
        assert_eq!(agents2.height, 1);
    }
```

- [ ] **Step 3: Rework `render_panes` signature + body**

Change the signature (drop `status_area`/`footer_area`/`multi_pane_footer`; rename to `bottom_area`/`label`/`agent`):

```rust
#[allow(clippy::too_many_arguments)]
pub fn render_panes(
    f: &mut Frame,
    panes: &[PaneSpec<'_>],
    dividers: &[Divider],
    chip_area: Rect,
    bottom_area: Rect,
    agents_area: Rect,
    label: &str,
    agent: Option<AgentKind>,
    attention_line: Option<Line<'static>>,
    pinned: &[PinnedCommand],
    diff: Option<crate::git::DiffStats>,
    pr: Option<(BranchLifecycle, u32)>,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active_agent: Option<AgentInstanceId>,
    theme: &Theme,
) -> PanesDrawOutput {
```

Replace the body region that rendered the attention line + footer (lines ~104-128) with the bottom-line render. Keep the dividers, chip-row, and agents-row rendering. The new sequence:

```rust
    render_dividers(f, dividers, theme);

    // Bottom line: agent bar + focused label, then attention items.
    let line = bottom_line(label, agent, attention_line, theme);
    f.render_widget(Paragraph::new(line), bottom_area);

    // Chip row (pinned + rule + diff/PR). The `^x: menu` hint is added in the
    // next task; for now no footer hints are emitted.
    let (chip_rects, pr_link_rect) = render_chip_row(f, chip_area, pinned, diff, pr, theme);

    let agent_chip_rects: Vec<(AgentInstanceId, Rect)> = if agents.is_empty() {
        Vec::new()
    } else {
        let spans = agents_row_spans(agents, active_agent, theme);
        f.render_widget(Paragraph::new(Line::from(spans)), agents_area);
        let rects = layout_agents_row(agents_area, agents);
        agents.iter().map(|(id, _, _, _)| *id).zip(rects).collect()
    };

    PanesDrawOutput {
        chip_rects,
        pr_link_rect,
        pane_rects,
        agent_chip_rects,
        footer_hint_rects: Vec::new(),
    }
}
```

Delete the `footer_line` import (line 21) and the now-unused `FooterHintSpan`/`key_for_glyph` imports from `mod.rs` if the compiler flags them.

- [ ] **Step 4: Update the Attached call site in `render.rs`**

In `src/app/render.rs` (the `View::Attached` arm, ~lines 399-577):

1. Compute the prefix width and shrink attention `max_width`. Replace lines ~403-411:

```rust
            let prefix_w =
                attached::bottom_line_prefix_width(&focused_label, focused_agent) as usize;
            let max_width = (area.width as usize).saturating_sub(3 + prefix_w);
            let attention = if matches!(app.modal, Some(crate::ui::modal::Modal::UpdatesPanel { .. })) {
                None
            } else {
                compute_attention_line(app, Some(focused_id), max_width)
            };
```

(`focused_label`/`focused_agent` are already in scope above this block; if the order means they're defined later, move their definitions above this line.)

2. Replace the `layout_chrome` call (lines ~468-474):

```rust
            let (pane_area, chip_area, bottom_area, agents_area) =
                attached::layout_chrome(area, agents_present);
```

3. Offset the attention rects by `prefix_w` and anchor to `bottom_area` (lines ~475-494):

```rust
            let attention_rects: Vec<(crate::data::store::WorkspaceId, ratatui::layout::Rect)> =
                attention
                    .as_ref()
                    .map(|a| {
                        a.segments
                            .iter()
                            .map(|s| {
                                (
                                    s.workspace_id,
                                    ratatui::layout::Rect {
                                        x: bottom_area
                                            .x
                                            .saturating_add(prefix_w as u16)
                                            .saturating_add(s.start_col),
                                        y: bottom_area.y,
                                        width: s.width,
                                        height: 1,
                                    },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
```

4. Update the `render_panes` call (lines ~552-570) to the new signature — drop `status_area`/`footer_area`/`multi_pane`, pass `bottom_area`:

```rust
            let out = attached::render_panes(
                f,
                &specs,
                &dividers,
                chip_area,
                bottom_area,
                agents_area,
                &focused_label,
                focused_agent,
                attention_line,
                &pinned,
                diff,
                pr,
                &focused_agents_list,
                active_agent,
                &app.theme,
            );
```

- [ ] **Step 5: Update the AttachedPm call site in `render.rs`**

In the `View::AttachedPm` arm (~lines 581-641):

```rust
                let prefix_w =
                    attached::bottom_line_prefix_width("project-manager", None) as usize;
                let max_width = (area.width as usize).saturating_sub(3 + prefix_w);
                let attention = if matches!(app.modal, Some(crate::ui::modal::Modal::UpdatesPanel { .. })) {
                    None
                } else {
                    compute_attention_line(app, None, max_width)
                };
                let pinned: &[crate::commands::pinned::PinnedCommand] = &[];
                let (pane_area, chip_area, bottom_area, agents_area) =
                    attached::layout_chrome(area, false);
```

Update its `attention_rects` block the same way (offset by `prefix_w`, anchor to `bottom_area`), and the `render_panes` call:

```rust
                let out = attached::render_panes(
                    f,
                    &specs,
                    &[],
                    chip_area,
                    bottom_area,
                    agents_area,
                    "project-manager",
                    None,
                    attention_line,
                    pinned,
                    None,
                    None,
                    &[],
                    None,
                    &app.theme,
                );
```

- [ ] **Step 6: Build + run the attached layout/render tests**

Run: `cargo test --lib attached`
Expected: PASS, including the rewritten `layout_chrome_reclaims_footer_rows`. Fix any leftover references to the old 5-tuple.

- [ ] **Step 7: Bottom-line render test**

Add to the `tests` module in `src/ui/attached/mod.rs`:

```rust
    #[test]
    fn bottom_line_prefix_width_matches_drawn_prefix() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = Theme::wsx();
        // Build a 1-entry attention line so we can locate where it starts.
        let attn = Line::from(vec![Span::raw("ATTN".to_string())]);
        let prefix = bottom_line_prefix_width("wsx/foo", Some(AgentKind::Claude)) as usize;
        let mut term = Terminal::new(TestBackend::new(60, 1)).unwrap();
        term.draw(|f| {
            let line = bottom_line("wsx/foo", Some(AgentKind::Claude), Some(attn.clone()), &theme);
            f.render_widget(Paragraph::new(line), Rect::new(0, 0, 60, 1));
        })
        .unwrap();
        let buf = term.backend().buffer();
        let row: String = (0..60).map(|x| buf[(x, 0)].symbol()).collect();
        // "ATTN" begins exactly at the prefix width.
        assert_eq!(&row[prefix..prefix + 4], "ATTN", "row={row:?}");
    }

    #[test]
    fn bottom_line_label_only_when_no_attention() {
        let theme = Theme::wsx();
        let line = bottom_line("wsx/foo", None, None, &theme);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "wsx/foo");
    }
```

- [ ] **Step 8: Run + commit**

Run: `cargo test --lib attached`
Expected: PASS.

```bash
git add src/ui/attached/mod.rs src/app/render.rs
git commit -m "feat(attached): drop footer chrome; bottom line shows label + attention"
```

---

## Task 7: `^x: menu` hint on the chip row

**Files:**
- Modify: `src/ui/attached/mod.rs` (`render_panes`: carve the menu-hint area, emit the ArmLeader hint)

- [ ] **Step 1: Carve the menu hint in `render_panes`**

Replace the chip-row render line from Task 6 with a version that paints the `^x` pill + ` menu` at the left of `chip_area`, records its clickable rect, and runs `render_chip_row` on the remainder:

```rust
    // `^x: menu` hint at the far left of the chip row: a `^x` key-pill + a
    // ` menu` label. Clickable — arms the leader (opens the overlay) exactly
    // like pressing Ctrl-x. The pinned chips/rule/right-block render in the
    // area to its right, unchanged.
    let menu_label = " menu";
    let hint_w = 3 + menu_label.chars().count() as u16; // " ^x " pill (3) + label
    let mut hint_spans: Vec<Span<'static>> = Vec::with_capacity(4);
    hint_spans.extend(key_pill_spans("^x", theme));
    hint_spans.push(Span::styled(menu_label.to_string(), Style::default().fg(theme.path)));
    f.render_widget(
        Paragraph::new(Line::from(hint_spans)),
        Rect { x: chip_area.x, y: chip_area.y, width: hint_w.min(chip_area.width), height: 1 },
    );
    let hint_rect = Rect {
        x: chip_area.x,
        y: chip_area.y,
        width: hint_w.min(chip_area.width),
        height: 1,
    };
    let footer_hint_rects = vec![(hint_rect, crate::ui::footer::FooterHintAction::ArmLeader)];

    // Chips render to the right of the hint (plus a 2-col gap).
    let chips_x = chip_area.x.saturating_add(hint_w + 2);
    let chips_area = Rect {
        x: chips_x,
        y: chip_area.y,
        width: chip_area.width.saturating_sub(hint_w + 2),
        height: 1,
    };
    let (chip_rects, pr_link_rect) = render_chip_row(f, chips_area, pinned, diff, pr, theme);
```

And change the returned struct to use the computed `footer_hint_rects`:

```rust
    PanesDrawOutput {
        chip_rects,
        pr_link_rect,
        pane_rects,
        agent_chip_rects,
        footer_hint_rects,
    }
```

(Re-add `use crate::ui::footer::FooterHintAction;` to `mod.rs` if needed, or use the fully-qualified path as above.)

- [ ] **Step 2: Hint-rect test**

Add to the `tests` module in `mod.rs` — render a single pane and assert the first cells spell the hint and a hint rect comes back. Use the existing `PaneSpec`/`Session` test scaffolding if present; otherwise assert via the geometry helper by checking `render_chip_row` is offset. Minimal geometry assertion:

```rust
    #[test]
    fn menu_hint_width_offsets_chips() {
        // The chips must start past the "^x menu" hint + 2-col gap.
        let theme = Theme::wsx();
        let hint_w = 3 + " menu".chars().count() as u16;
        let chip_area = Rect::new(0, 5, 80, 1);
        let chips_area = Rect {
            x: chip_area.x + hint_w + 2,
            y: chip_area.y,
            width: chip_area.width - (hint_w + 2),
            height: 1,
        };
        // A pinned chip's first rect sits within chips_area, never under the hint.
        let pinned = [crate::commands::pinned::PinnedCommand {
            label: "pr".into(),
            command: "/pr".into(),
        }];
        let rects = chip_row::layout_chip_row(chips_area, &pinned);
        assert!(rects[0].x >= chip_area.x + hint_w + 2, "chip clears the hint");
    }
```

(If `layout_chip_row` is not visible from `mod.rs` tests, qualify as `chip_row::layout_chip_row` — it is `pub` in that submodule.)

- [ ] **Step 3: Build + run**

Run: `cargo test --lib attached`
Expected: PASS.

- [ ] **Step 4: Manual sanity (optional)**

Run the app, attach to a workspace, confirm: bottom row shows `▎ <label>` then attention; chip row starts with `^x menu`; Ctrl-x opens the centered overlay; ↑↓ moves the marker; Enter/letters fire; Esc closes. (See `/run` skill for launching.)

- [ ] **Step 5: Commit**

```bash
git add src/ui/attached/mod.rs
git commit -m "feat(attached): ^x menu hint on the chip row (arms the overlay)"
```

---

## Task 8: Retire the always-on footer + final verification

**Files:**
- Delete: `src/ui/attached/footer.rs`
- Modify: `src/ui/attached/mod.rs` (`mod footer;` + `use footer::footer_line;` removal)

- [ ] **Step 1: Remove the footer module**

Delete `src/ui/attached/footer.rs`. In `src/ui/attached/mod.rs` remove `mod footer;` and `use footer::footer_line;`. Build and fix any dangling references:

Run: `cargo build`
Expected: compiles. If `key_for_glyph`/`FooterHintSpan` are now unused in `mod.rs`, drop those imports. `key_for_glyph` is still used by `nav_menu.rs` (via `crate::ui::footer::key_for_glyph`) and the `FooterHintAction`/`FooterHintSpan` types still live in `src/ui/footer.rs` — only the attached `footer.rs` submodule is removed.

- [ ] **Step 2: Full test suite**

Run: `cargo test`
Expected: all pass. Some `app::input` PTY-timing tests can flake under the full run — re-run any failure in isolation before treating it as a regression (`cargo test <name>`).

- [ ] **Step 3: Lint + format (CI gates on both)**

Run:
```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
Expected: clean. Run `cargo fmt` if `--check` reports diffs, then re-stage.

- [ ] **Step 4: Commit**

```bash
git add src/ui/attached/footer.rs src/ui/attached/mod.rs
git commit -m "refactor(attached): remove always-on keybind footer (now in ^x overlay)"
```

---

## Self-Review Notes

- **Spec coverage:** chip-row `^x: menu` hint (Task 7); bottom line = label + attention (Task 6); footer→overlay (Tasks 2-5, 8); menu + accelerators with keybind-first layout (Tasks 2-4); single source of truth + Enter parity (Tasks 2-3); PM parity (Tasks 5-6 call sites + `pm_nav_menu_items`); agents row untouched (Task 6 preserves it). All covered.
- **Known minor deviation:** with the overlay open, ↑↓ drive the highlight, so multi-pane *vertical* (stacked) focus via Up/Down-after-^x is no longer arrow-reachable; Left/Right still focus and the `←→ focus pane` row covers it via Enter. Acceptable per the design's interaction model.
- **Type consistency:** `layout_chrome` 5-tuple→4-tuple and the 2-arg signature are updated at both call sites; `render_panes` drops `status_area`/`footer_area`/`multi_pane_footer`; `bottom_line_prefix_width` is the shared width source for draw + rect offset.
