# Agent Color Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a fixed-color `▎` bar identifying each workspace's coding agent (claude=orange, pi=purple, hermes=yellow, codex=blue), as a new leftmost column on dashboard rows and in the attached-view chrome.

**Architecture:** A new `Theme::agent_style(AgentKind) -> Style` returns a theme-independent fixed RGB. The dashboard row renderer (`row.rs`) gains an `agent` field on `RowInputs` and draws a `▎` bar as column 0, left of the existing status gutter, producing a two-tone left edge. The attached view threads the agent through `PaneSpec` and the footer to draw the same bar.

**Tech Stack:** Rust, ratatui (TUI), existing `AgentKind` enum (`src/pty/session.rs`), per-renderer unit tests.

**Spec:** `docs/superpowers/specs/2026-06-02-agent-color-indicator-design.md`

---

## File Structure

- `src/ui/theme.rs` — add four fixed agent-color `const`s + `Theme::agent_style`. Owns all color decisions.
- `src/ui/dashboard/row.rs` — add `agent` field to `RowInputs`, `AGENT_WIDTH` const, render column 0, fix two existing gutter tests that now index the wrong span, add new tests.
- `src/app/render.rs` — set `agent: ws.agent` on the production dashboard `RowInputs`; thread agent into the attached `PaneSpec` list and the footer.
- `src/ui/attached.rs` — add `agent: Option<AgentKind>` to `PaneSpec`, draw the bar in the per-pane title bar, add an `agent` param to `footer_line`/`render_panes`, add tests.
- Test/fixture `RowInputs` sites get `agent: AgentKind::Claude`: `src/ui/dashboard/by_repo.rs`, `src/ui/dashboard/by_attention.rs` (two sites), `src/ui/dashboard/tests.rs`.

---

## Task 1: Agent color mapping on `Theme`

**Files:**
- Modify: `src/ui/theme.rs` (imports at top; new consts after imports; new method in `impl Theme`; tests in the `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block at the bottom of `src/ui/theme.rs`:

```rust
    #[test]
    fn agent_style_maps_each_kind_to_fixed_rgb() {
        use crate::pty::session::AgentKind;
        let t = Theme::wsx();
        assert_eq!(
            t.agent_style(AgentKind::Claude).fg,
            Some(Color::Rgb(0xe8, 0x8b, 0x3c))
        );
        assert_eq!(
            t.agent_style(AgentKind::Pi).fg,
            Some(Color::Rgb(0xa9, 0x7b, 0xd6))
        );
        assert_eq!(
            t.agent_style(AgentKind::Hermes).fg,
            Some(Color::Rgb(0xf0, 0xd0, 0x66))
        );
        assert_eq!(
            t.agent_style(AgentKind::Codex).fg,
            Some(Color::Rgb(0x5b, 0x9d, 0xe0))
        );
    }

    #[test]
    fn agent_colors_are_theme_independent() {
        use crate::pty::session::AgentKind;
        let a = Theme::wsx();
        let b = Theme::dracula();
        for agent in AgentKind::ALL {
            assert_eq!(a.agent_style(agent).fg, b.agent_style(agent).fg);
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib agent_style_maps_each_kind_to_fixed_rgb agent_colors_are_theme_independent`
Expected: FAIL — compile error, `no method named agent_style found for struct Theme`.

- [ ] **Step 3: Add the import, color constants, and method**

At the top of `src/ui/theme.rs`, change the first `use` line:

```rust
use crate::pty::session::AgentKind;
use ratatui::style::{Color, Modifier, Style};
```

Immediately after the imports (before `#[derive(Debug, Clone, Copy)] pub struct Theme`), add the constants:

```rust
/// Fixed per-agent identity colors. Independent of the active theme so the
/// agent a workspace runs on stays recognizable when the user switches
/// themes — agent identity is constant, unlike the status/lifecycle tones
/// which each theme re-skins.
const AGENT_CLAUDE: Color = Color::Rgb(0xe8, 0x8b, 0x3c); // orange
const AGENT_PI: Color = Color::Rgb(0xa9, 0x7b, 0xd6); // purple
const AGENT_HERMES: Color = Color::Rgb(0xf0, 0xd0, 0x66); // yellow
const AGENT_CODEX: Color = Color::Rgb(0x5b, 0x9d, 0xe0); // blue
```

Inside `impl Theme`, next to `status_style`, add:

```rust
    /// Fixed identity color for a workspace's coding agent. Ignores `self`
    /// by design — see the `AGENT_*` constants — but lives on `Theme` so
    /// every color decision stays in one module, alongside `status_style`
    /// and `lifecycle_style`.
    pub fn agent_style(&self, agent: AgentKind) -> Style {
        let fg = match agent {
            AgentKind::Claude => AGENT_CLAUDE,
            AgentKind::Pi => AGENT_PI,
            AgentKind::Hermes => AGENT_HERMES,
            AgentKind::Codex => AGENT_CODEX,
        };
        Style::default().fg(fg)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib agent_style_maps_each_kind_to_fixed_rgb agent_colors_are_theme_independent`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add src/ui/theme.rs
git commit -m "feat(theme): add fixed per-agent color mapping"
```

---

## Task 2: Agent bar column on dashboard rows

**Files:**
- Modify: `src/ui/dashboard/row.rs` (imports, `RowInputs`, width consts, `render`, `base()` fixture, two existing tests, new tests)
- Modify: `src/app/render.rs:84` (production `RowInputs`)
- Modify: `src/ui/dashboard/by_repo.rs:133` (test `RowInputs`)
- Modify: `src/ui/dashboard/by_attention.rs` (two test `RowInputs`, ~lines 252 and 408)
- Modify: `src/ui/dashboard/tests.rs:42` (test `RowInputs`)

- [ ] **Step 1: Write the failing tests**

In `src/ui/dashboard/row.rs`, add to the `mod tests` block:

```rust
    #[test]
    fn agent_bar_is_leftmost_span_with_agent_color() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Pi;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let first = line.spans.first().expect("agent bar present");
        assert_eq!(first.content.as_ref(), "▎");
        assert_eq!(first.style.fg, theme.agent_style(AgentKind::Pi).fg);
    }

    #[test]
    fn agent_bar_precedes_status_gutter_as_two_tone_edge() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Codex; // blue
        inputs.status = Status::Complete; // green gutter — distinct from blue
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(line.spans[0].content.as_ref(), "▎", "agent bar first");
        assert_eq!(line.spans[1].content.as_ref(), "▎", "status gutter second");
        assert_eq!(
            line.spans[0].style.fg,
            theme.agent_style(AgentKind::Codex).fg
        );
        assert_eq!(
            line.spans[1].style.fg,
            theme.status_style(Status::Complete).fg
        );
        assert_ne!(line.spans[0].style.fg, line.spans[1].style.fg);
    }

    #[test]
    fn agent_bar_keeps_color_when_selected() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Hermes;
        inputs.selected = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(line.spans[0].content.as_ref(), "▎");
        assert_eq!(
            line.spans[0].style.fg,
            theme.agent_style(AgentKind::Hermes).fg
        );
        assert_eq!(
            line.spans[1].content.as_ref(),
            "▍",
            "status gutter still thickens on selection"
        );
    }

    #[test]
    fn ago_stays_right_aligned_after_agent_column() {
        let theme = Theme::wsx();
        let line = render(&base(), ColumnWidths::default(), 0, &theme, 120);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.trim_end().ends_with("29s ago"),
            "age column stays right-aligned: {text:?}"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib agent_bar_is_leftmost_span_with_agent_color`
Expected: FAIL — compile error, `RowInputs` has no field `agent` and `AgentKind` is not in scope.

- [ ] **Step 3: Add the import and the `agent` field**

At the top of `src/ui/dashboard/row.rs`, add to the imports:

```rust
use crate::pty::session::AgentKind;
```

In the `RowInputs` struct, add the field next to `status` (order doesn't matter, but keep it first for readability):

```rust
pub struct RowInputs {
    pub agent: AgentKind,
    pub status: Status,
    pub name: String,
    // ... rest unchanged
}
```

- [ ] **Step 4: Add the width const and update `base()`**

Add the width constant next to the other column-width consts (after `const GLYPH_WIDTH: usize = 2;`):

```rust
const AGENT_WIDTH: usize = 1;
```

In the `base()` test fixture, add the field (matching the struct order, first):

```rust
    fn base() -> RowInputs {
        RowInputs {
            agent: AgentKind::Claude,
            status: Status::Question,
            // ... rest unchanged
```

- [ ] **Step 5: Render the agent bar as column 0**

In `render`, immediately before the existing `// 1: gutter` block (the `let gutter_glyph = ...` line), insert:

```rust
    // 0: agent identity bar — a fixed per-agent color, independent of
    // status. Sits left of the status gutter so the row shows a two-tone
    // left edge: outer = agent, inner = status. Plain Unicode, no
    // nerd-font gating (same glyph as the gutter).
    spans.push(Span::styled(
        "▎".to_string(),
        theme.agent_style(inputs.agent),
    ));
```

In the `left_consumed` calculation, add `AGENT_WIDTH` as the first term:

```rust
    let left_consumed = AGENT_WIDTH
        + GUTTER_WIDTH
        + ELBOW_WIDTH
        + GLYPH_WIDTH
        + name_width
        + branch_width
        + PROCS_WIDTH
        + DIFF_WIDTH;
```

- [ ] **Step 6: Fix the two existing gutter tests that now target the wrong span**

The status gutter is now `spans[1]`, not `spans[0]`. Update these two existing tests in `row.rs`.

In `unselected_row_uses_thin_gutter_glyph`, change:

```rust
        let gutter = line.spans.first().expect("gutter span present");
```
to:
```rust
        let gutter = line.spans.get(1).expect("status gutter span present");
```

In `selected_row_uses_thicker_gutter_glyph`, change:

```rust
        let gutter = line.spans.first().expect("gutter span present");
```
to:
```rust
        let gutter = line.spans.get(1).expect("status gutter span present");
```

(The `renders_design_columns_in_order` test asserts `text.starts_with("▎")` — still true, the agent bar is also `▎` — so it needs no change.)

- [ ] **Step 7: Update the production `RowInputs` site**

In `src/app/render.rs`, in the dashboard `RowInputs { ... }` block (~line 84), add the field as the first entry:

```rust
                    let row = crate::ui::dashboard::row::RowInputs {
                        agent: ws.agent,
                        status,
                        name: ws.name.clone(),
                        // ... rest unchanged
```

- [ ] **Step 8: Update the test/fixture `RowInputs` sites**

Add `agent: crate::pty::session::AgentKind::Claude,` (or `AgentKind::Claude` where already imported) as the first field in each of these `RowInputs { ... }` literals:

- `src/ui/dashboard/by_repo.rs` (~line 133, inside `make_view`)
- `src/ui/dashboard/by_attention.rs` (~line 252, inside `make_rows`)
- `src/ui/dashboard/by_attention.rs` (~line 408, inside `flat_row_renders_repo_slash_workspace_in_name`)
- `src/ui/dashboard/tests.rs` (~line 42)

For each, the import may not be present; use the fully-qualified path `crate::pty::session::AgentKind::Claude` to avoid touching imports:

```rust
                row: RowInputs {
                    agent: crate::pty::session::AgentKind::Claude,
                    status: w.status,
                    // ... rest unchanged
```

- [ ] **Step 9: Run the row tests to verify they pass**

Run: `cargo test --lib dashboard::row`
Expected: PASS — all existing row tests plus the four new ones.

Then build the whole lib to confirm every `RowInputs` site compiles:

Run: `cargo test --lib --no-run`
Expected: compiles with no errors.

- [ ] **Step 10: Commit**

```bash
git add src/ui/dashboard/row.rs src/app/render.rs src/ui/dashboard/by_repo.rs src/ui/dashboard/by_attention.rs src/ui/dashboard/tests.rs
git commit -m "feat(dashboard): add per-agent color bar to workspace rows"
```

---

## Task 3: Agent bar in the attached view

**Files:**
- Modify: `src/ui/attached.rs` (`PaneSpec`, `render_one_pane` title bar, `footer_line` + `render_panes` signatures, existing `footer_line` test call, new tests)
- Modify: `src/app/render.rs` (multi-pane `pane_data`/`PaneSpec` build + `render_panes` call; `AttachedPm` `PaneSpec` + `render_panes` call)

- [ ] **Step 1: Write the failing tests**

In `src/ui/attached.rs`, add to the `mod tests` block:

```rust
    #[test]
    fn footer_line_prepends_agent_bar_when_present() {
        let theme = Theme::wsx();
        let line = footer_line("wsx/foo", Some(AgentKind::Codex), false, &theme);
        assert_eq!(line.spans[0].content.as_ref(), "▎");
        assert_eq!(line.spans[0].style.fg, theme.agent_style(AgentKind::Codex).fg);
        assert_eq!(
            line.spans[2].content.as_ref(),
            "wsx/foo",
            "label follows the bar and its trailing space"
        );
    }

    #[test]
    fn footer_line_omits_agent_bar_when_none() {
        let theme = Theme::wsx();
        let line = footer_line("project-manager", None, false, &theme);
        assert_eq!(
            line.spans[0].content.as_ref(),
            "project-manager",
            "no leading bar for the PM pane"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib footer_line_prepends_agent_bar_when_present`
Expected: FAIL — compile error: `footer_line` takes 3 arguments / `AgentKind` not in scope.

- [ ] **Step 3: Import `AgentKind` and add the `PaneSpec` field**

At the top of `src/ui/attached.rs`, change:

```rust
use crate::pty::session::Session;
```
to:
```rust
use crate::pty::session::{AgentKind, Session};
```

Add the field to `PaneSpec`:

```rust
pub struct PaneSpec<'a> {
    pub session: &'a Arc<Session>,
    pub label: &'a str,
    pub rect: Rect,
    pub focused: bool,
    /// The pane's coding agent, or `None` for the project-manager pane
    /// (which is not one of the four coding agents).
    pub agent: Option<AgentKind>,
}
```

- [ ] **Step 4: Draw the agent bar in the per-pane title bar**

In `render_one_pane`, replace the `let spans = vec![ ... ];` block (the two-element vec with the gutter and label) with:

```rust
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(3);
        if let Some(agent) = pane.agent {
            // Agent identity bar, left of the focus gutter → two-tone edge.
            spans.push(Span::styled("▎".to_string(), theme.agent_style(agent)));
        }
        spans.push(Span::styled("▎".to_string(), gutter_style));
        spans.push(Span::styled(format!(" {} ", pane.label), name_style));
```

- [ ] **Step 5: Add the `agent` param to `footer_line`**

Change the `footer_line` signature:

```rust
fn footer_line(
    label: &str,
    agent: Option<AgentKind>,
    multi_pane: bool,
    theme: &Theme,
) -> Line<'static> {
```

Inside, replace the line that pushes the label span:

```rust
    spans.push(Span::styled(label.to_string(), theme.header_style()));
```
with:
```rust
    if let Some(a) = agent {
        spans.push(Span::styled("▎".to_string(), theme.agent_style(a)));
        spans.push(Span::raw(" ".to_string()));
    }
    spans.push(Span::styled(label.to_string(), theme.header_style()));
```

- [ ] **Step 6: Add the `footer_agent` param to `render_panes` and pass it through**

Change the `render_panes` signature to add `footer_agent: Option<AgentKind>` immediately after `footer_label`:

```rust
    footer_area: Rect,
    footer_label: &str,
    footer_agent: Option<AgentKind>,
    multi_pane_footer: bool,
```

Inside `render_panes`, update the `footer_line` call:

```rust
        footer_line(footer_label, footer_agent, multi_pane_footer, theme),
```

- [ ] **Step 7: Fix the existing `footer_line` test call**

In `src/ui/attached.rs`, the test `footer_line_pill_wraps_key_only_not_label` calls `footer_line(...)`. Add `None` as the second argument:

```rust
        let line = footer_line("label", None, false, &theme);
```

(If the existing call uses different arguments, keep them and insert `None` as the new second positional argument.)

- [ ] **Step 8: Run the attached tests to verify they pass**

Run: `cargo test --lib attached::`
Expected: PASS — new footer tests plus the updated existing test.

- [ ] **Step 9: Thread the agent through the multi-pane call site**

In `src/app/render.rs`, in the `View::Attached` arm:

(a) After `focused_label` is computed (~line 369), add the focused agent lookup:

```rust
            let focused_agent = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == focused_id)
                .map(|(_, w)| w.agent);
```

(b) Change the `pane_data` tuple type and population to carry the agent. Replace the `pane_data` binding (the `let pane_data: Vec<(...)> = panes ...` block) with:

```rust
            let pane_data: Vec<(
                std::sync::Arc<crate::pty::session::Session>,
                String,
                ratatui::layout::Rect,
                bool,
                Option<crate::pty::session::AgentKind>,
            )> = panes
                .into_iter()
                .filter_map(|(ws_id, path, rect)| {
                    let session = app.sessions.get(ws_id)?;
                    let (label, agent) = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == ws_id)
                        .map(|(_, w)| (w.name.clone(), Some(w.agent)))
                        .unwrap_or_default();
                    let focused = path == state.focus;
                    Some((session, label, rect, focused, agent))
                })
                .collect();
```

(c) Update the `specs` map to set `agent`:

```rust
            let specs: Vec<crate::ui::attached::PaneSpec<'_>> = pane_data
                .iter()
                .map(|(s, l, r, f, a)| crate::ui::attached::PaneSpec {
                    session: s,
                    label: l.as_str(),
                    rect: *r,
                    focused: *f,
                    agent: *a,
                })
                .collect();
```

(d) Add `focused_agent` to the `render_panes` call, immediately after `&focused_label,`:

```rust
            let out = attached::render_panes(
                f,
                &specs,
                &dividers,
                chip_area,
                status_area,
                footer_area,
                &focused_label,
                focused_agent,
                multi_pane,
                attention_line,
                &pinned,
                &app.theme,
            );
```

- [ ] **Step 10: Thread `None` through the `AttachedPm` call site**

In the `View::AttachedPm` arm of `src/app/render.rs`:

(a) Add `agent: None` to the PM `PaneSpec`:

```rust
                let specs = [crate::ui::attached::PaneSpec {
                    session,
                    label: "project-manager",
                    rect: pane_area,
                    focused: true,
                    agent: None,
                }];
```

(b) Add `None` to its `render_panes` call, after the `"project-manager",` label argument:

```rust
                let out = attached::render_panes(
                    f,
                    &specs,
                    &[],
                    chip_area,
                    status_area,
                    footer_area,
                    "project-manager",
                    None,
                    false,
                    attention_line,
                    pinned,
                    &app.theme,
                );
```

- [ ] **Step 11: Build and run the full lib test suite**

Run: `cargo test --lib`
Expected: PASS — whole library compiles and all unit tests pass.

- [ ] **Step 12: Commit**

```bash
git add src/ui/attached.rs src/app/render.rs
git commit -m "feat(attached): show agent color bar in pane titles and footer"
```

---

## Task 4: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Format check**

Run: `cargo fmt --all`
Then: `git diff --stat` — review any formatting fixups. If files changed, `git add -A && git commit -m "style: rustfmt"`.

- [ ] **Step 2: Lint**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any that appear (e.g. unused import) and re-run.

- [ ] **Step 3: Full test suite (lib + integration)**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 4: Manual smoke check (optional but recommended)**

Run `wsx` against a setup with workspaces on different agents and confirm:
- Dashboard rows show a two-tone left edge (`▎▎`): outer agent color, inner status color.
- Orange = claude, purple = pi, yellow = hermes, blue = codex.
- Attaching to a workspace shows the agent-colored bar before the footer label; multi-pane titles show it before the focus gutter.
- The project-manager pane shows no agent bar.

---

## Notes for the implementer

- The bar glyph is `▎` (U+258E, LEFT ONE QUARTER BLOCK) — the exact char already used for the status gutter. Copy it; don't retype from a description.
- `ratatui::style::Color` implements `PartialEq`, so `assert_eq!` on `.style.fg` works (see existing `diff_cell_colors_...` test).
- `AgentKind` is `Copy`, so `w.agent` / `*a` move-free copies are fine.
- The color RGBs are starting values from the spec; they can be nudged later without any structural change — only the four `AGENT_*` consts in `theme.rs` and the two assertions in `agent_style_maps_each_kind_to_fixed_rgb` move together.
