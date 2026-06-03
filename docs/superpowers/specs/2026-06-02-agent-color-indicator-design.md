# Agent color indicator — design

## Problem

wsx now spawns one of four coding agents per workspace (Claude, Pi, Hermes,
Codex). Today the dashboard rows and attached-view chrome give no signal as
to *which* agent is driving a given workspace. When you scan the dashboard or
zoom into a pane, you can't tell a Claude workspace from a Codex one.

## Goal

Add a fixed-color indicator that identifies the agent for each workspace:

| Agent  | Color  |
|--------|--------|
| Claude | orange |
| Pi     | purple |
| Hermes | yellow |
| Codex  | blue   |

The indicator is a thin vertical bar `▎` placed as a new **leftmost column**,
immediately to the left of the existing status-colored gutter bar. This
produces a two-tone left edge on every row: the outer bar carries the agent
color, the inner bar carries the status color.

```
▎▎├  ⠋ payments-api      ⎇ bakedbean/...
^ ^
| └ status gutter (e.g. purple = thinking)
└ agent gutter (orange = claude)
```

## Decisions (from brainstorming)

- **Glyph:** thin bar `▎`, twin to the existing status gutter. Plain Unicode
  (already used for the gutter), so no nerd-font gating.
- **Scope:** dashboard rows (both by-repo and by-attention views) **and** the
  attached-view header chrome (per-pane title bar + footer label).
- **Colors:** fixed RGB constants, identical across all themes — agent
  identity stays recognizable when the user switches themes.

## Color values

Fixed `ratatui::style::Color::Rgb` constants. Chosen to be mutually
distinguishable — in particular Claude's orange must read as distinctly
*oranger* than Hermes' yellow, and not collide with the existing amber
`question`/`warn` status color.

| Agent  | RGB        | Hex       |
|--------|------------|-----------|
| Claude | 232,139,60 | `#e88b3c` |
| Pi     | 169,123,214| `#a97bd6` |
| Hermes | 240,208,102| `#f0d066` |
| Codex  | 91,157,224 | `#5b9de0` |

These are starting values; they can be nudged after seeing them live without
changing the design.

## Architecture

The agent→color mapping and all rendering changes follow the existing seams.

### 1. Color mapping — `src/ui/theme.rs`

Add a method on `Theme`:

```rust
pub fn agent_style(&self, agent: AgentKind) -> Style
```

It returns `Style::default().fg(<fixed rgb>)`, ignoring `self` (colors are
theme-independent by decision). Keeping it on `Theme` co-locates it with
`status_style` and `lifecycle_style` so every color decision lives in one
module. The four RGB constants are defined as `const`s in `theme.rs`.

Rationale for not adding per-theme tokens: the user chose fixed colors, so
threading four new fields through all five `Theme` constructors would be pure
overhead.

### 2. Dashboard rows — `src/ui/dashboard/row.rs`

- Add `agent: AgentKind` to `RowInputs`.
- Add `const AGENT_WIDTH: usize = 1;`.
- Render a new **column 0** before the existing gutter: a `▎` span styled with
  `theme.agent_style(inputs.agent)`. The agent bar keeps its color regardless
  of selection (mirrors how the status gutter keeps its status color when
  selected). Unlike the status gutter, the agent bar does **not** thicken on
  selection — selection is already signalled by the status gutter and the bg
  tint, and a single steady agent bar keeps the two-tone edge legible.
- Include `AGENT_WIDTH` in the `left_consumed` total that drives the flex
  message-column width, so all downstream columns stay aligned.

### 3. Row construction sites

`RowInputs` is built in one production path and several test/fixture paths:

- **Production:** `src/app/render.rs` (~line 84) — set `agent: ws.agent`
  (`Workspace` already carries `agent: AgentKind`).
- **Tests/fixtures:** `src/ui/dashboard/by_repo.rs`, `by_attention.rs`,
  `row.rs` (`base()`), `tests.rs` — set `agent: AgentKind::Claude` (the
  existing fixtures don't model agent; Claude is the natural default and
  matches their existing `AgentKind::Claude` usage elsewhere).

### 4. Attached view — `src/ui/attached.rs`

- Add `agent: Option<AgentKind>` to `PaneSpec`. `Option` because the
  project-manager pane (`AttachedPm` view) is not one of the four coding
  agents — `None` renders no agent bar.
- **Per-pane title bar** (`render_one_pane`, multi-pane only): prepend an
  agent-colored `▎` bar before the existing focus gutter, giving the same
  two-tone left edge as the dashboard. Skip when `agent` is `None`.
- **Footer label** (`footer_line` / `render_panes`): the footer corresponds to
  the focused workspace. Prepend the agent-colored `▎` bar (plus a space)
  before the workspace label span. Thread the focused pane's
  `Option<AgentKind>` into `render_panes` for this. Skip when `None`.

### 5. Thread agent into the attached call site — `src/app/render.rs`

The `pane_data` tuple (`session, label, rect, focused`) gains the workspace's
`agent` (looked up alongside `label` from `app.workspaces`). The `AttachedPm`
spec passes `agent: None`. The footer's agent is the focused pane's agent.

## Data flow

```
Workspace.agent ─┬─ render.rs (dashboard) ─→ RowInputs.agent ─→ row::render ─→ ▎ agent bar
                 └─ render.rs (attached)  ─→ PaneSpec.agent  ─→ title bar + footer ─→ ▎ agent bar
                                                                      │
                                          Theme::agent_style(agent) ──┘ (fixed RGB)
```

## Testing

Follow the existing pure-renderer test style in `row.rs`:

- `agent_bar_is_first_span_with_agent_color`: for each `AgentKind`, the first
  span is `▎` and its `fg` equals `theme.agent_style(agent).fg`.
- `agent_bar_precedes_status_gutter`: rendered line starts with `▎▎` (agent
  then status), and the two spans carry different colors for a non-orange
  status.
- `agent_bar_keeps_color_when_selected`: selected row still shows the agent
  color on column 0 (and the status gutter still thickens to `▍`).
- `left_consumed_accounts_for_agent_column`: a regression guard that the
  message/age columns stay aligned (e.g. the line still ends with the ago
  string) after adding the column.
- `theme.rs`: `agent_style_maps_each_kind` — each `AgentKind` maps to its
  fixed RGB, and the mapping is identical across two different themes.
- `attached.rs`: title bar with `Some(agent)` renders a leading agent-colored
  `▎`; with `None` (PM pane) it does not.

## Out of scope (YAGNI)

- Hiding the indicator when only one agent kind is in use — always-on is
  simpler and predictable.
- Per-theme agent palettes — fixed RGB by decision.
- Colorblind-alternative encodings (glyph differentiation per agent).
- A legend/help affordance explaining the color code.
