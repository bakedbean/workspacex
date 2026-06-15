# Attached-view navigation modal + reclaimed bottom line

## Problem

In the attached (agent chat) view, the keybind navigation is rendered as an
always-on cheat-sheet footer at the bottom of the screen (the `^x` leader pill
followed by `d detach`, `u updates`, `a agents`, `e/t/v/g` external tools,
`k procs`, `x send-^x`, plus multi-pane `close-pane` / `←→ focus`). This
permanently consumes two rows of chrome. Separately, the "workspaces that need
attention" line sits on its own row just above the footer.

We want to:

1. Hide the always-on cheat-sheet and instead surface the navigation as a
   centered modal overlay (Docker-TUI style) when the user arms the `^x`
   leader.
2. Reclaim the footer rows by moving the current workspace + attention items
   onto the single line the footer used to occupy.

## Background: how navigation works today

The `^x` leader already exists. Pressing `Ctrl-x` sets `app.leader_pending`;
the next key is dispatched by `handle_key_attached` (and `handle_key_attached_pm`
for the PM pane). The footer drawn by `footer_line`
(`src/ui/attached/footer.rs`) is purely a *cheat-sheet* for that chord — the
keys already fire whether or not the footer is visible.

Chrome is laid out by `layout_chrome` in `src/ui/attached/mod.rs` as a vertical
stack: pane (`Min 1`), chip row (`1`), attention row (`0|1`), footer (`2` =
spacer + keys), agents row (`0|1`). `compute_attention_line`
(`src/app/render.rs`) produces the attention content; `render_panes` wires it
all together.

Because the keys already work via the leader, this change is mostly
**presentation**: stop drawing the cheat-sheet footer, draw an overlay instead,
and re-home the attention line. The underlying key dispatch stays.

## Design

### 1. Chip row — `^x: menu` hint

Prepend a `^x: menu` pill to the far left of the chip row, before the
pinned-command chips. It is clickable and arms the leader (opens the menu),
reusing the existing `FooterHintAction::ArmLeader`. The rest of the chip row
(pinned chips, `─` rule filler, right-justified diff/PR block) is unchanged.

### 2. Bottom line — workspace + attention

The two footer rows collapse into a single, always-present row that hosts:

- **Left:** the agent identity bar `▎` (agent color) + the focused workspace
  label, exactly the identity the footer used to show. This keeps the workspace
  name visible in single-pane mode, where there is no per-pane title bar.
- **After the label:** the workspaces-needing-attention content from
  `compute_attention_line`, given the width budget remaining after the label.
  When nothing needs attention, nothing renders after the label.

### 3. Cheat-sheet footer removed

`footer_line` is no longer rendered as always-on chrome. Its action list is
re-used as the source for the overlay (below). The two-row footer rect is
removed from `layout_chrome`.

### 4. Ctrl-x navigation overlay

While `leader_pending` is set in the attached view, draw a centered, bordered
panel (matching the `panel_frame` look of the other modals). Keybind column
first, then label column:

```
┌─ actions ──────────────────┐
│ ▌ d   detach               │
│   u   updates              │
│   a   agents               │
│   e   edit                 │
│   t   open terminal        │
│   v   diff                 │
│   g   lazygit              │
│   k   processes            │
│   x   send literal ^x      │
│ ↑↓ move · enter · esc      │
└────────────────────────────┘
```

- Contents are context-aware. Multi-pane mode adds `d close-pane` (replacing
  `detach`) and `←→ focus`. The PM pane lists only its smaller `d/x/u` set.
- The pinned commands `1-9` still fire via the leader; they are already visible
  as chips, so the panel shows a single `1-9 run pinned` hint line rather than
  enumerating each.
- The agent-switch keys (`q/w/r/…`) are **not** in this panel — the multi-agent
  `agents:` switcher row stays exactly where it is and keeps owning them.

**Interaction (menu + accelerators):**
- `↑↓` move the highlight (the `▌` marker).
- `Enter` fires the highlighted action.
- Pressing the action letter fires it immediately (preserves today's muscle
  memory).
- `Esc` or a second `Ctrl-x` dismisses.

### 5. Input

The overlay is keyed off the existing `leader_pending` state — it is not a new
`Modal` variant — so letter accelerators keep working unchanged. We add:

- `app.leader_selected: usize` — the highlight index, reset to 0 when the
  leader is armed and clamped to the current context's action count.
- `↑↓` while `leader_pending` adjust `leader_selected`.
- `Enter` while `leader_pending` maps `leader_selected` to its action key and
  dispatches it.

To guarantee the drawn menu and the dispatch never drift, the leader action
match in `handle_key_attached` is factored into a single
`dispatch_leader_action(app, key)` helper, and the menu's `(key, label)` list
is the single source of truth that both the renderer and the `Enter` handler
consume. `Enter` resolves `items[leader_selected].key` and calls the same
helper the letter path uses.

### 6. Scope

Applies to both the coding-agent attached view (`handle_key_attached`) and the
PM pane (`handle_key_attached_pm`); they share the `render_panes` /
`layout_chrome` / leader path. The PM overlay lists its smaller action set. The
multi-agent `agents:` switcher row is unchanged.

## Components touched

- `src/ui/attached/mod.rs` — `layout_chrome` (drop footer rows; attention/
  bottom row always present, 1 tall), `render_panes` (compose bottom line =
  label + attention; stop rendering the cheat-sheet footer).
- `src/ui/attached/footer.rs` — repurpose the action list as the menu source of
  truth; remove always-on rendering.
- `src/ui/attached/chip_row.rs` — prepend the `^x: menu` hint pill.
- New: nav-overlay renderer (centered panel listing the context's actions with
  the highlight marker).
- `src/app/render.rs` — draw the overlay when `leader_pending`; adjust the
  attention width budget for the label prefix.
- `src/app/input.rs` — `dispatch_leader_action` helper; `↑↓` / `Enter` handling
  while `leader_pending`; reset `leader_selected` on arm.
- `src/app/mod.rs` (or wherever `App` lives) — add `leader_selected: usize`.

## Testing

- `layout_chrome`: footer rows reclaimed; chip + bottom-line + optional agents
  rows tile the area without overlap; bottom line is always 1 tall.
- Bottom line composes the agent bar + label + attention with correct width
  budgeting (attention truncates against the post-label width; empty attention
  renders just the label).
- Nav-menu item list is the single source of truth: rendered order equals
  dispatch order; `Enter` at index *i* fires the same action as pressing
  `items[i].key`.
- Context variants: multi-pane adds `close-pane` + `←→ focus`; PM lists only
  `d/x/u`.
- The `^x: menu` chip hint arms the leader on click (emits
  `FooterHintAction::ArmLeader`).

## Out of scope / non-goals

- No change to the dashboard's own `^x` leader (pinned chips / `a` agents) or
  the `z` fold-leader.
- No change to which actions exist or what they do — only how navigation is
  presented and where the attention line lives.
- The multi-agent `agents:` switcher row keeps its current position and keys.
