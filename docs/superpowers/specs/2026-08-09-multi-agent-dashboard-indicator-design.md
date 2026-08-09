# Dashboard multi-agent indicator

**Date:** 2026-08-09
**Status:** Designed

## Problem

A workspace can hold several agent instances — `workspace_agents` stores one row per
instance and `instance_label` names them `claude`, `claude#2`, `codex` — but the
dashboard row shows no sign of it. Column 0 renders a single `▎` colored by
`RowInputs.agent`, which is the denormalized `workspaces.agent` column, i.e. the
*primary's* kind only. A workspace running a peer reviewer looks exactly like one
running nothing but its primary.

The row is a fixed-width budget, not a canvas. At a 120-col terminal the flex recap
column gets ~41 columns and ~33 after the status token, so an indicator that costs
width unconditionally comes straight out of the recap that PR #267 just made useful.

## Solution overview

Widen the existing agent bar into a right-aligned **agent strip**: one `▎` per live
agent instance, each in its own kind color, primary rightmost. More agents reads as a
thicker, multi-colored left edge — scannable without interaction and without any new
visual vocabulary, since `▎` colored by `theme.agent_style` already means "agent" in
the attached footer (`agents_row.rs:42-47`), the agents panel, and the pane title bar.

The strip's width is **adaptive**: `clamp(max live-agent count across the rows about to
render, 1, 4)`. A dashboard with no peers anywhere is byte-identical to today and costs
nothing.

```
   ▎▎├  …  ⎇ add-widgets-api   ⏺ #12 open   ● 2p   └ working · api      4m
  ▎▎▎├  ✓  ⎇ fix-auth-drift    ⏺ #14 open   ● 1p   └ done · auth        1m
 ▎▎▎▎├  ?  ⎇ audit-invoices    ·            ● 3p   └ asking · CV-049…   9s
 +▎▎▎├  …  ⎇ big-fanout-ws     ⏺ #17 open   ● 5p   └ working · 5 peers  1m
```

Row 1 is primary-only, row 2 is claude + codex, row 3 adds pi, row 4 overflows.

## Decisions and their alternatives

Three forks were settled during design; recording them so the plan doesn't relitigate.

**Signal: count + kinds**, not a boolean. A single heavier `▌` bar would cost zero
columns but conveys only "has peers", and `▌` already means "focused agent" in the
attached footer. A branch-column badge (`◈2`) would cost nothing on single-agent rows
but eats branch-name characters on precisely the busiest workspaces, and that badge
stack already carries layout, shared, and setup markers.

**Width: adaptive**, not fixed-3 and not a user setting. Fixed-3 is perfectly stable but
taxes every row 2 recap columns forever even with zero peers. A `dashboard_agent_width`
setting following the `dashboard_branch_width` precedent was rejected as an unneeded
knob — it would also need an entry in `known_setting_key()`. The accepted cost of
adaptive is that the list shifts sideways when a peer appears or exits.

**Dead peers: excluded.** Nothing auto-reaps an instance when its session exits —
`remove_workspace_agent` has exactly one non-test call site, the manual `x` key at
`input.rs:1893`. So a one-shot reviewer stays registered forever, and under adaptive
width a *registered*-based strip would stay permanently widened for an agent that no
longer exists. Counting live sessions lets the width self-heal. Auto-reaping on exit was
rejected as scope creep that also destroys the peer's scrollback and its
`agent_messages` inbox rows (`schema.rs:280`). The full roster stays visible in the
agents panel (`^x a`) and `wsx agent list`.

## 1. Strip rendering

In `src/ui/dashboard/row.rs`:

- `AGENT_WIDTH` (row.rs:39) stops being a const and becomes `ColumnWidths.agent`
  (row.rs:50-57), alongside the existing `branch` and `pr` fields.
- Bars render right-aligned within `widths.agent`, left-padded with spaces: peers
  ordered by `created_at` ascending, then the primary last. A newly added peer therefore
  lands adjacent to the primary and older peers shift left.
- Each bar is `▎` (U+258E) styled `theme.agent_style(kind)` — unchanged from today for
  the primary.
- Overflow (live count exceeds `widths.agent`, only reachable above 4): the leftmost
  cell renders a dim `+` and the remaining cells hold the most recent peers plus the
  primary. Five live agents is reachable with one keystroke — the agents-panel `a` key
  adds all four kinds at once (`input.rs:1877-1883`).
- Padding cells are spaces that must carry the selected-row background so the highlight
  doesn't gap on the left edge.

**The primary bar is never liveness-gated.** It renders in its kind color whether or not
a session is running, exactly as today; only peer bars require a `Running` session. This
keeps the change strictly additive — a workspace that has never been attached looks
identical to now rather than losing its bar.

All glyphs are single-width. This is load-bearing: `display_width` is literally
`s.chars().count()` (row.rs:508-510), so a double-width glyph would misalign every
column to its right. No nerd-font gating is needed — `▎` and `+` are plain
Unicode/ASCII, matching how the existing bar is drawn.

## 2. Width computation

`RowInputs` (row.rs:70-96) keeps `agent: AgentKind` (the primary) and gains
`peers: Vec<AgentKind>`, holding live peers only in `created_at` order.

`ColumnWidths.agent` is **derived, not read from settings**, so `read_column_widths`
(`app/render.rs:939-954`) does not populate it — it keeps reading only
`dashboard_branch_width` and `dashboard_pr_width`, and leaves `agent` at its 1 default.

The derived width is computed in **`render_by_repo` (`mod.rs:442`) and
`render_by_attention` (`mod.rs:524`)**, not inside the view modules. This placement is
load-bearing: each of those functions builds the visible row set, then uses
`inputs.column_widths` *twice* — once in the PR-chip hit-test walk (`mod.rs:508`,
`mod.rs:608`) and once when calling `render_list` (`mod.rs:519`, `mod.rs:618`).
Computing the width where both callers can see it keeps rendering and hit-testing
consistent by construction. Computing it lower down, inside `render_list`, would widen
the drawn row while the hit-test walk still used the unwidened value — silently
offsetting every PR-chip click target.

Both functions have already applied the user's filter and (for `by_repo`) the fold state
at that point, which is what makes "max across visible rows" correct. `by_attention`
already clones `RowInputs`, so it inherits `peers` for free.

## 3. Downstream width consumers

Two places compute x offsets from the column widths and **both** must switch from
`AGENT_WIDTH` to `widths.agent`:

- `left_consumed` (row.rs:284-292) — feeds `message_width` for the flex column.
- `pr_chip_hit_span` (row.rs:373-378) — recomputes the chip's x offset *independently*
  of `left_consumed`. Missing it makes PR-chip mouse clicks land on the wrong column,
  silently, with every existing test still passing.

The module doc-comment (row.rs:5-14) is already stale — it omits the agent bar
entirely. Update it to the real column table while editing this file.

## 4. Data plumbing

`Session` (`pty/session.rs:89-112`) has no `workspace_id` — `spawn()` receives one but
only feeds it to `SpawnIdentity` — so the instance-to-workspace mapping must come from
SQLite regardless.

**Extract `App::live_instances(ws_id) -> Vec<AgentInstance>`:** roster fetch via
`store.workspace_agents(ws_id)` plus a filter to instances whose `app.sessions` entry
has `SessionStatus::Running`. This idiom is already duplicated at `app.rs:790-801` and
`app.rs:1982-1994`, so the extraction collapses existing duplication rather than adding
speculative abstraction. Convert both call sites.

`render.rs:504` looks similar but is **not** the same idiom and must be left alone — the
attached footer's agent switcher deliberately lists every *registered* instance,
unfiltered by liveness, so exited agents stay switchable and clickable.

**Cache the roster** as `App::agent_roster: HashMap<WorkspaceId, Vec<AgentInstance>>`,
populated in `App::refresh()` alongside `app.workspaces`, so the per-frame render path
does no DB I/O. Liveness is read from the in-memory `app.sessions` each frame.

The cache is filled by a new bulk query `Store::all_workspace_agents() ->
HashMap<WorkspaceId, Vec<AgentInstance>>`, mirroring `all_workspace_status`
(`data/status.rs:119-134`) — one statement for the whole table rather than one per
workspace. `refresh()` already bulk-loads `pushed_status` and
`workspaces_with_multi_pane_layouts` this way (`app.rs:733-765`), so this follows the
established shape.

Every mutation path must invalidate the cache:

- `input.rs:1879` — agents-panel add (and the `a` key that adds all four kinds)
- `input.rs:1893` — agents-panel remove
- `wsx agent add` from a separate process — picked up on the next `refresh()`

`refresh()` is known to be called after the share toggle (`app.rs:1996`); whether it
already fires on the add/remove keys is unverified and is part of the implementation
work. If it does not, add it.

## 5. Edge cases

- **No live sessions at all** — strip is one cell holding the primary's bar, i.e. today's
  behavior. Covered by the primary-not-gated rule in section 1.
- **Peer registered but never started** — no `app.sessions` entry, so not `Running`, so
  excluded. Same path as an exited peer.
- **Selected row** — padding spans carry the selected background.
- **Live count above the cap** — dim `+` marker, per section 1.
- **Adaptive shift** — adding or exiting a peer moves the whole list sideways by a
  column. Accepted, and self-explanatory in motion.

## 6. Scope

Dashboard row and its two views only. The agents panel, attached-view footer, pane title
bar, PM pane, macOS menubar, waybar, and CLI are unchanged.

No schema change. Per-agent `working`/`waiting`/`blocked`/`done` does not exist —
`workspace_status` and `workspace_recap` are both `PRIMARY KEY workspace_id` — and this
design does not need it. Per-agent liveness (`SessionStatus`) and idle time
(`activity_ms`) already exist; only liveness is used.

## 7. Testing

- A single-agent row at agent width 1 renders byte-identically to today.
- Right-alignment, padding, and fixed total width across 1–4 agents; `+` overflow at 5
  live agents.
- Column alignment holds as the strip widens — extend the pattern of
  `unshared_row_has_no_shared_badge_and_widths_stay_aligned` (row.rs:658-694).
- **`pr_chip_hit_span` at agent width > 1** — guards the silent click-target regression
  from section 3.
- Width computation: clamp to 1..4, exited peers excluded, primary counted even with no
  session, max taken over visible rows only.
- `App::live_instances` — includes running peers, excludes exited and never-started
  ones, and the three converted call sites keep their existing behavior.
- CI gates per repo convention: `cargo fmt --check` (rustfmt 1.95.0 via
  `mise exec rust@1.95.0`), clippy, `cargo test`.
