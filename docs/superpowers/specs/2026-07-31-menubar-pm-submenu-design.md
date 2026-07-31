# Project Manager submenu for the macOS menubar — design

Date: 2026-07-31
Status: approved pending spec review

## Goal

Surface the Project Manager digest — the agent-authored `goal` / `state` /
`next` recap each workspace maintains — in the macOS SwiftBar menubar, as a
`Project Manager` submenu listed below the workspace rows. Selecting it opens
a vertical, natively-scrolling menu showing the same per-workspace narrative
the TUI's `p` view shows.

See `2026-07-09-pm-digest-design.md` for the recap data model and the TUI
digest, and `2026-07-29-macos-menubar-design.md` for the menubar
architecture this extends.

## Non-goals

- No new window, browser view, or standalone TUI process. The submenu is the
  whole surface.
- No change to the plugin's cache-only contract: no git, `gh`, or session-log
  I/O on the render path.
- No third level of SwiftBar nesting. The per-workspace action submenu (Open
  PR, Copy path, Reveal in Finder) stays on the top-level workspace rows,
  where it already lives.
- No new refresh sweep. Recap data is written by agents via `wsx recap set`
  and read straight from SQLite; there is nothing to recompute.

## Constraint: the process boundary

The TUI digest (`App::build_pm_digest`, `src/app.rs`) is assembled from
in-memory caches — `workspace_events` (session JSONL tailing), live
`git::WorkspaceStatus`, `pr_lifecycle`. The SwiftBar plugin is a short-lived
process that reads SQLite only.

Of the digest's fields:

| Field | Available to the plugin? |
|---|---|
| recap goal / state / next | Yes — `workspace_recap` table |
| status state / message / age | Yes — `workspace_status` table |
| PR number + lifecycle | Yes — `scm_cache` |
| dirty, `+adds -dels` | Yes — `scm_cache` |
| `↑ahead ↓behind` | **No** — not in `scm_cache` |
| `active 4m ago` | **No** — needs session-log tailing |
| `recap stale` marker | **No** — derived from session-log activity |

Resolutions:

- **ahead/behind** is dropped. The menubar's own vocabulary is dirty +
  diffstat, so the PM fact line stays consistent with the workspace rows
  above it.
- **`active 4m ago`** becomes **`recap 2h`**, the age of
  `recap.updated_at` — already loaded, and in a Project Manager view the
  recap's own age is the more relevant clock.
- **`recap stale`** is dropped entirely. A 2h-old recap next to a status
  reported 4m ago tells the reader the same thing without wsx asserting a
  verdict from a proxy signal.

## Rendering

### Placement

The PM section is appended by `render()` after the repo/workspace body and
before the existing footer:

```
<header line>
---
<repo headers + workspace rows + their action submenus>
---
Project Manager
-- <PM submenu lines>
---
Refresh | refresh=true
```

The `Project Manager` line carries no action params. An item followed by
`--` children is a submenu parent; it is left enabled (not
`disabled=true`) so the submenu opens cleanly on hover.

The quiet state is unchanged: `document()` still returns the icon-only
header when no repos are registered, so no PM section renders at all.

### Submenu body

All lines are at `--` depth. Per repo, in the same repo order as the main
menu:

```
-- <repo name> | disabled=true
-- <glyph> <slug>  <state> <age> | bash=<wsx> param1="menubar" param2="jump" …
--<NBSP><NBSP>goal:  <goal> | disabled=true font=<ROW_FONT>
--<NBSP><NBSP>state: <state> | disabled=true font=<ROW_FONT>
--<NBSP><NBSP>next:  <next> | disabled=true font=<ROW_FONT>
--<NBSP><NBSP><facts> | disabled=true font=<ROW_FONT>
-----
```

- The **header line** is the only clickable item, running the existing
  `wsx menubar jump <repo> <slug>`. Its text is `<state_glyph> <slug>`,
  plus — when a pushed status exists — that status's state word and age
  (e.g. `blocked 4m`), reusing `state_glyph` from `workspace_rows` and
  `format_age` for the age. The status *message* is not repeated here; it
  already appears in the workspace row's own action submenu, and the recap
  lines are the narrative this view exists for.
- **Recap lines** render one per non-empty field. A workspace counts as
  **having a recap** only if its `workspace_recap` row exists and at least
  one of `goal` / `state` / `next` is present and non-empty; a row whose
  three fields are all NULL or empty is treated exactly as if absent. A
  workspace without a recap renders a single `no recap yet` line instead.
- The **facts line** joins the present segments with ` · `: PR field (reusing
  `pr_field`), `●` when dirty, `+a -d` when either is non-zero, and
  `recap <age>` when the workspace has a recap by the definition above.
  Omitted entirely when no segment applies.
- Indentation uses **U+00A0 (NBSP)**, not spaces — SwiftBar trims leading
  whitespace from menu text. `ROW_FONT` (`SFMono-Regular size=12`) is
  fixed-width, so the `goal:` / `state:` / `next:` labels align.
- `-----` separates workspace blocks (a `---` separator at `--` depth).
  Whether SwiftBar honours this is the one thing that needs manual
  verification; the fallback is a `disabled=true` line containing a single
  NBSP.

### Empty states

- A repo with no Ready workspaces renders `-- (no workspaces) |
  disabled=true`, matching the main menu's existing behavior for empty
  repos.
- A workspace whose recap row is absent renders `no recap yet` in place of
  the three recap lines. Facts and header still render.

### Ordering

Within each repo, cards sort by `(attention_rank, signal_ms)` ascending:

- `attention_rank`: `Blocked` → 0, `Waiting` → 1, everything else → 2.
  This mirrors the TUI's `pm_pane::attention_rank`, **not** the menubar's
  existing `workspace_rows::attention_rank` (which ranks descending and
  places `Done` above `Waiting` — it exists to pick the header's worst
  state and is not reused here).
- `signal_ms`: `max(recap.updated_at, status.reported_at)`, or `0` when
  neither exists — the last time the agent said anything, oldest first. A
  recap row treated as absent (all three fields empty) contributes no
  `updated_at`, keeping ordering consistent with what the card renders.
  This is the DB-side proxy for the TUI's stalest-first activity tiebreak,
  and `0` for "never" reproduces the TUI's own `unwrap_or(0)`, floating
  never-seen workspaces to the top of their rank.

### Membership

The PM section covers **exactly the workspaces the main menu lists** — every
workspace `store.workspaces()` returns, in every state (`Pending`, `Ready`,
`Failed`, `Orphaned`).

This deliberately diverges from the TUI digest, which shows `Ready` only.
Two reasons: `RowInput` carries no workspace state (`workspace_metas` never
reads it), so filtering would mean threading a new field through the shared
Linux/macOS row module purely to hide rows; and the hidden rows would still
be visible in the same dropdown a few pixels above. Divergent *ordering*
between the two sections is the point of the feature; divergent
*membership* would just be confusing.

## Escaping and truncation

Recap text is agent-authored and passes through the same injection barrier
as status messages: control characters collapse to spaces (`sanitize`), `|`
becomes `¦` (`esc_core`), and a leading `-` becomes `‑` so no value can read
as a separator or submenu marker (`esc_text`).

The existing `MAX_TEXT_LEN = 120` cap is too loose for this surface — NSMenu
sizes itself to its widest item, so one long goal line widens the entire
dropdown. Recap lines get `MAX_RECAP_LEN = 72`, applied with a trailing `…`
when truncation occurs. Doctrine already specifies one-liners, so
truncation should be rare. Param values (paths, URLs) remain uncapped, as
today.

## Time units

`workspace_recap.updated_at` and `workspace_status.reported_at` are epoch
**milliseconds**; `scm_cache.fetched_at` / `git_fetched_at` are epoch
**seconds**. PM code touches only the former two and must use
`crate::time::now_ms()`, never `workspace_rows::unix_now()` — which is in
the very module it imports `RowInput` from.

## Code layout

| File | Change |
|---|---|
| `src/menubar/escape.rs` | **New.** `esc_core`, `esc_text`, `quote_param`, and new `esc_text_capped(s, max)` (which `esc_text` delegates to with `MAX_TEXT_LEN`). Moved out of `plugin.rs` with their existing tests, so `pm.rs` can share them without `plugin.rs` becoming a utility module. |
| `src/menubar/pm.rs` | **New.** `PmCard`, `build_pm_cards`, `pm_section_lines`, `MAX_RECAP_LEN`, and the module's tests. |
| `src/menubar/mod.rs` | Declare `escape` and `pm`. |
| `src/workspace_rows.rs` | `RowInput` gains `id: WorkspaceId`. `collect_rows_cached` currently discards it, leaving nothing to join recaps against. Populated in both collect paths; the waybar formatter ignores it. |
| `src/menubar/plugin.rs` | `plugin_document` also loads `store.all_workspace_recaps()`; `render()` takes the recap map and appends `pm_section_lines`. Escaping helpers now come from `escape`. `pr_field` and `ROW_FONT` stay here and are shared with `pm.rs` as `pub(crate)`. |
| `src/time.rs` | `format_age` moves here from `src/ui/updates_bar.rs`, which re-exports it so TUI call sites are unchanged. A pure duration formatter should not live in a ratatui widget module once a non-TUI caller needs it. |

Render-path cost: one additional SQLite query
(`all_workspace_recaps`). No subprocesses. Document growth is ~5 lines per
workspace.

## Testing

Unit tests in `src/menubar/pm.rs`:

- Ordering: blocked before waiting before the rest; within a rank, oldest
  `signal_ms` first; a card with neither recap nor status sorts to the top
  of its rank.
- A populated card renders header, three recap lines, and a facts line
  containing PR field, `●`, diffstat, and `recap <age>`.
- A card with no recap row renders `no recap yet` and no `recap <age>`
  segment.
- A recap row whose `goal` / `state` / `next` are all NULL or empty is
  treated identically to a missing row — `no recap yet`, no `recap <age>`,
  and no `updated_at` contribution to `signal_ms`.
- A card with only some recap fields set renders only those lines.
- Workspaces in every state (`Pending`, `Failed`, `Orphaned`) appear, matching
  the main menu's membership.
- Only the header line carries a `bash=` jump action; every other line is
  `disabled=true`.
- A repo with no workspaces renders `(no workspaces)`.
- Recap text containing `\n`, `|`, and a leading `-` is neutralized, and no
  submenu line contains a newline.
- A 200-char goal is capped to `MAX_RECAP_LEN` with an ellipsis.
- Recap lines are indented with NBSP, not ASCII spaces.

In `src/menubar/plugin.rs`:

- `render()` places the PM section after the last repo and before the
  `Refresh` footer, preceded by a `---`.
- `document()` with no repos is still icon-only — no PM section.

In `src/time.rs`: `format_age` keeps its existing behavior (the tests move
with it).

Manual test added to `docs/manual-tests/menubar.md`: open the menubar,
confirm the `Project Manager` submenu opens, shows recaps in
attention order, jumps on click, and — the one thing unit tests cannot
answer — that `-----` renders as a separator inside the submenu rather than
as literal text. If it renders literally, fall back to an NBSP
`disabled=true` line.

## Commits

1. `refactor(menubar): extract SwiftBar escaping into escape.rs; carry workspace id on RowInput`
2. `refactor: move format_age to time.rs`
3. `feat(menubar): Project Manager submenu with per-workspace recaps`
4. `docs: menubar PM submenu manual test + README note`

## Trade-offs accepted

- The menubar PM view is a near-copy of the TUI digest, not an exact one:
  no ahead/behind, no last-activity age, no stale marker. The cache-only
  contract is worth more than field-for-field parity.
- Recap quality still depends on agent doctrine compliance — unchanged from
  the TUI digest.
- The submenu restates workspaces already listed above it. The different
  ordering (attention-first) and the narrative lines are what earn it the
  space; if it proves redundant in daily use, the fix is to narrow its
  scope, not to change the surface.
