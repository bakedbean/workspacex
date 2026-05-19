# V5 Dashboard — grouped list with status vocabulary

## Background

The current dashboard (`src/ui/dashboard/mod.rs`) renders workspaces as a
single list with a sub-line per row. Activity is expressed via 9 ad-hoc
labels (`awaiting / question / complete / stalled / active / idle /
waiting / resumable / off`) that grew organically from the classifier in
`app.rs`. Repo headers are a flat `name · path · count` line followed by
a full-width rule.

A new design (`~/Desktop/design_handoff_wsx_dashboard/`) defines V5: a
grouped single-pane list with a **canonical 6-state status vocabulary**,
a **status strip** between the chrome and the list, **rich workspace
rows** (status gutter, glyph, branch, procs, line-diff, message,
relative time), and a **by-attention alternate view** for triage. Spec
includes exact glyphs, oklch color values, column widths, sort rules,
and animation cadence.

## Goal

Replace the current dashboard renderer with V5 end-to-end:

- Collapse the 9-label classifier output into the design's 6 canonical
  statuses (`Question / Stalled / Waiting / Thinking / Complete / Idle`).
- Add the status strip and repo-header counts cluster.
- Add the by-attention view (toggle with `g`) and per-repo fold (`z`).
- Add a persistent 24h activity sparkline in the footer.
- Compute `+N −N` line-count diffs and render them in a fixed column.
- Extend every theme with status colors; add a new default `wsx` theme
  that matches the design tokens exactly.

Existing decorations survive on top of V5: PR lifecycle styling on the
branch column, YOLO warn-style on the name, the `⚙!` setup-failed
badge, and nerd-font glyphs when enabled.

## Non-goals

- No PM-pane changes. When `pm_visible`, the dashboard still takes the
  top 60% of the area; PM keeps the bottom 40%.
- No new persistent fold state. Folds live in memory only for v1.
- No responsive reflow for very narrow terminals (< 100 cols). Standard
  ratatui clipping applies at narrower widths.
- No filter UI persistence; `/` filter state resets on view switch.
- No richer thinking-vs-waiting differentiation beyond what the existing
  classifier already distinguishes. (Today's `active < 2s` and
  `waiting ≥ 30s` thresholds keep their meanings.)

## Status vocabulary (canonical 6)

```rust
// src/ui/dashboard/status.rs
pub enum Status { Question, Stalled, Waiting, Thinking, Complete, Idle }
```

Mapping from the existing classifier inputs:

| classifier input | → Status | glyph | live? |
|---|---|---|---|
| `awaiting_tool.is_some()` (permission prompt ≥3s) | `Question` | `?` | no |
| `stopped_kind = AwaitingAnswer` | `Question` | `?` | no |
| `stalled = true` (no JSONL appends >60s, no pending tool, stop_reason observed) | `Stalled` | `!` | no |
| `stopped_kind = Complete` | `Complete` | `✓` | no |
| running, `secs < 2` | `Thinking` | `⠋` (spin) | yes |
| running, `2 ≤ secs < 30` | `Thinking` | `⠋` (spin) | yes |
| running, `secs ≥ 30` | `Waiting` | `…` (spin) | yes |
| no session, has prior | `Idle` | `·` | no |
| no session, no prior | `Idle` | `·` | no |

`Status::classify(...)` lives in `status.rs` and takes the same inputs
`classify_activity_with_events()` takes today. Priority for sorting:

```
Stalled(5) > Question(4) > Waiting(3) > Thinking(2) > Complete(1) > Idle(0)
```

Live states (`Thinking`, `Waiting`) render with a braille spinner whose
frame is selected by `(app.tick / 8) % 8`. The existing `Tick` event
fires every 16ms (~60 fps); dividing by 8 gives ~7.8 fps, matching the
spec's 8 fps target.

## Layout

```
Length(1)   top chrome      — wsx · dashboard      group: repo  attention    N repos · M workspaces
Length(1)   status strip    — ? 2 question  ! 1 stalled  … 2 waiting  ⠋ 2 thinking  ✓ 2 complete  · 3 idle
Length(1)   spacer rule     — blank line
Min(0)      main list       — by-repo or by-attention view
Length(1)   footer chrome   — ↑↓ nav  ↵ open  z fold  n new  …  v0.4.2  24h <sparkline>
```

`pm_visible` still splits the screen 60/40; the dashboard's internal
layout above operates inside the top 60%.

### Top chrome

```
wsx · dashboard      group: repo  attention                    N repos · M workspaces
```

- `wsx` in `header_style()` (bold).
- ` · dashboard` in `dim`.
- After ~24 spaces: `group:` in `faint`, then two tabs `repo` and
  `attention`. Active tab uses `selected_style()`; inactive in `muted`.
- Right-edge: `N repos · M workspaces` in `muted`.

Pressing `g` (or clicking a tab in mouse mode — out of scope for v1)
toggles the group mode.

### Status strip

```
? 2 question   ! 1 stalled   … 2 waiting   ⠋ 2 thinking   ✓ 2 complete   · 3 idle
```

Each cell renders `<glyph> <count> <label>` with the glyph + count in
the status color (bold), label in `muted`. Cells with `count == 0`
still render but in `faint` so the strip has a fixed shape and width.
Cell separator is 3 spaces. Strip background uses the theme's `bg_alt`
(when the theme provides one; otherwise default bg).

### Main list

The body of the screen — populated by either `by_repo::render(...)` or
`by_attention::render(...)` depending on `DashboardState::group_mode`.

### Footer

```
↑↓ nav  ↵ open  z fold  n new  e edit  t tmux  v diff  r reply  g group  / filter  q quit          v0.4.2  24h ▂▃▄▅▆▆▇█
```

Keybind tokens use the existing `dim_style()`. Right-edge: `v0.4.2`
from `env!("CARGO_PKG_VERSION")` in `muted`, then `24h ` in `faint`,
then 24 sparkline blocks rendered from `app.activity_history` (see
"Activity sparkline" below).

## By-repo view

### Repo header

One line per repo, content + horizontal rule + counts on a single row:

```
▾ wsx  /home/eben/workspace/wsx ─────────────  ? 1  ! 1  … 1  ⠋ 0  ✓ 1  · 0   4 ws
```

Spans (left → right):

- 1ch fold indicator: `▾` expanded, `▸` folded, ` ` if zero workspaces.
- repo name in `header_style()`.
- 2 spaces, repo path in `faint`.
- 2 spaces.
- `─` runs filling to `right_cluster_start`.
- 2 spaces.
- Per-status `<glyph> <n>` tokens, separated by 2 spaces. Only render
  cells where `n > 0`. `Question` / `Stalled` bold; others normal
  weight; `Idle` faint.
- 4 spaces.
- Right-edge: `<N> ws` (or `no workspaces` when zero), in `faint`.

Pressing `z` while the selection is on a workspace toggles fold for
that workspace's repo. Pressing `z` while on a header also toggles
that header.

### Workspace row (under repo header)

Single line per workspace — no sub-line.

| col | width | content | style |
|---|---|---|---|
| 1 | 1ch | `▎` gutter | status color |
| 2 | 3ch | `├ ` elbow | `faint` |
| 3 | 2ch | status glyph or spinner frame | status color |
| 4 | 24ch | workspace name (left-aligned, ellipsized with `…`) | `bold`; `warn` if `yolo`; `accent` (blue) if selected; suffix `⚙!` in `err` if `setup_status == Failed` (truncates name to 21ch in that case) |
| 5 | 28ch | `⎇ <branch>` (`branch` icon nerd-font swap when enabled) | `faint` by default; PR lifecycle color override when set (`PrOpen=ok`, `PrConflicted=warn`, `PrMerged=merged`, `PrClosed=err`) |
| 6 | 6ch | `● Np` when procs > 0, else `  ·` | thinking-color dot + dim count; `faint` zero |
| 7 | 12ch | `+N −N` line diff | `faint`; blank when `DiffStats` not yet computed |
| 8 | flex | `└ <message>`; `—` when no message (idle / unknown) | `└ ` in row's status color; message in `dim`; `—` in `faint` |
| 9 | 10ch right | `Ns ago` / `Nm ago` / `Nh ago` | `faint`; `—` when no last-active |

Selected row: full-width `bg_sel` fill. Name span shifts to `accent`
+ bold.

### Sorting (by-repo)

- Repos sorted by **noise score** `question*100 + stalled*80 +
  waiting*40 + thinking*20 + complete*1`. Higher first; empty repos go
  to the bottom.
- Within each repo: sort workspaces by `Status` priority (Stalled at
  top, Idle at bottom).

### Folding (by-repo)

- Empty repos: default-folded.
- Repos with `question + stalled + waiting + thinking == 0`:
  default-folded.
- All other repos: default-expanded.
- User override (`z`) persists in `DashboardState::folded:
  HashMap<RepoId, bool>` for the session. `true` = explicitly folded;
  `false` = explicitly expanded; absent = default rule above.
- Lost on restart in v1. Persistent fold is a follow-up.

## By-attention view

Toggle with `g`. Drops repo grouping. Sections, each prefixed by an
`AttnHead` rule line:

| section | filter | header style |
|---|---|---|
| `◆ NEEDS ATTENTION` | `Status ∈ {Question, Stalled, Waiting}`, sorted by priority | `question` color, bold, uppercase letter-spacing |
| `● WORKING` | `Status == Thinking` | `thinking` color, bold |
| `✓ RECENT` | `Status == Complete` | `complete` color, bold |
| `  IDLE` | `Status == Idle && session_exists` | `muted` |
| `  QUIET REPOS` | repos with 0 workspaces or all `Idle` | `muted`, one summary line per repo |

`AttnHead` format:

```
◆ NEEDS ATTENTION  5 sessions ──────────────────────────────────── sorted by urgency
```

- Glyph + label + count on the left.
- `─` rule fills.
- Optional meta on the right (`sorted by urgency`, `live`, etc.) in
  `muted`.

Row format in this view changes column 4 (name) to `<repo>/<workspace>`,
36ch wide:

- `repo` in `muted`
- `/` in `faint`
- `workspace` in `bold` (or `accent` if selected)

Quiet-repos row format:

```
▎  ·  ssk      /home/eben/ssk/ssk-web                    5 idle
▎  ·  frontend /home/eben/meals/frontend                 no workspaces · press n to create
```

`▎` in idle color (which is faint), then status glyph `·`, then 18ch
repo name in `dim bold`, then 36ch path in `faint`, then a free-form
suffix.

### Sorting (by-attention)

- Within `◆ NEEDS ATTENTION`: by `Status` priority then by
  `last_active_at` desc.
- Within `● WORKING`, `✓ RECENT`, `  IDLE`: by `last_active_at` desc.
- Quiet repos: alphabetical.

## Keybindings

| key | action | new? |
|---|---|---|
| `↑` `↓` `j` `k` | move selection (skip headers in by-repo; wrap sections in by-attention) | unchanged |
| `↵` | open selected workspace | unchanged |
| `g` | toggle group mode (repo ↔ attention) | new |
| `z` | toggle fold on containing/selected repo | new |
| `r` | jump to reply on selected `Question` workspace (attaches + focuses claude prompt; no-op for other statuses) | new |
| `/` | enter filter mode (substring match on repo/workspace/branch/message); `Esc` clears | new |
| `n` `N` `e` `t` `v` `k` `s` `d` `q` | unchanged | — |

Filter applies after sorting; preserves the section structure. Empty
sections in by-attention view collapse when a filter is active.

## Theme extension

Add 6 fields to `Theme`:

```rust
pub struct Theme {
    // ... existing fields ...
    pub question: Color,
    pub stalled:  Color,
    pub waiting:  Color,
    pub thinking: Color,
    pub complete: Color,
    pub idle:     Color,
}
```

Each existing constructor sets them in-palette:

| theme | question | stalled | waiting | thinking | complete | idle |
|---|---|---|---|---|---|---|
| `ansi` (was `default`) | Yellow | Red | Blue | Magenta | Green | DarkGray |
| `dracula` | `#f1fa8c` | `#ff5555` | `#8be9fd` | `#bd93f9` | `#50fa7b` | `#6272a4` |
| `jellybeans` | `#ffb964` | `#cf6a4c` | `#8197bf` | `#c6b6ee` | `#99ad6a` | `#888888` |
| `nord` | `#ebcb8b` | `#bf616a` | `#88c0d0` | `#b48ead` | `#a3be8c` | `#4c566a` |
| `wsx` (new default) | `#e4ba6c` | `#d36258` | `#6ea7d8` | `#b78cd0` | `#67c089` | `#7a7e85` |

`wsx` RGB values are the oklch tokens from the design converted offline.

Add a `pub fn status_style(&self, s: Status) -> Style` helper that
maps a status to `Style::default().fg(<the right field>)`.

Rename: the existing `Theme::default_theme()` becomes
`Theme::ansi()`, exposed as `Theme::by_name("ansi")`. `Theme::default()`
now returns the new `wsx` theme. Update the four existing snapshot
tests in `theme.rs::tests` to match.

## Diff stats data source

New struct in `src/git.rs`:

```rust
pub struct DiffStats { pub added: u32, pub removed: u32 }

pub async fn workspace_diff_stats(
    worktree: &Path,
    base: &str,
) -> Option<DiffStats>;
```

Implementation: shell out to `git -C <worktree> diff --shortstat
<base>...HEAD` and parse the `N insertions(+), M deletions(-)` tail.
Returns `None` on any error (missing base, empty repo, etc.). Parser
must handle:

- `5 files changed, 32 insertions(+), 12 deletions(-)` → `(32, 12)`.
- `1 file changed, 18 insertions(+)` → `(18, 0)`.
- `2 files changed, 4 deletions(-)` → `(0, 4)`.
- ` (empty)` → `Some((0, 0))`.

Base branch source: `repo.base_branch: Option<String>` (already on
`Repo` per the existing schema). When `None`, `workspace_diff_stats`
returns `None` and the column renders blank. (The standalone
`wsx repo set-base-branch` CLI already lets users set this per repo.)

Wire into `app.rs`: in the per-workspace git polling loop where
`workspace_status` is refreshed, also compute `workspace_diff_stats`
and store in `app.workspace_diff: HashMap<WorkspaceId, DiffStats>`.
Render column 7 reads from that map.

## Activity sparkline (persistent)

24 hourly buckets covering the last 24 hours. Each bucket stores the
**max concurrency of live workspaces** (`Status ∈ {Thinking,
Waiting}`) observed during that hour.

### Storage

New sqlite table:

```sql
CREATE TABLE IF NOT EXISTS activity_buckets (
    hour_epoch INTEGER PRIMARY KEY,  -- unix epoch seconds, truncated to hour
    max_live   INTEGER NOT NULL
);
```

Plus pruning: on every insert, delete rows where `hour_epoch < now - 24h`.

### Update loop

In the main App tick handler:

```rust
let now_hour = current_hour_epoch();
let live = count_live_workspaces();
let prev = app.activity_history.back().copied();
if prev.map(|(h, _)| h) == Some(now_hour) {
    // Same hour — update max.
    let (_, prev_max) = app.activity_history.pop_back().unwrap();
    app.activity_history.push_back((now_hour, prev_max.max(live)));
} else {
    // New hour — persist previous bucket, start new one.
    if let Some((h, m)) = prev { store.set_activity_bucket(h, m)?; }
    app.activity_history.push_back((now_hour, live));
    store.prune_activity_buckets_before(now_hour - 24 * 3600)?;
}
```

### Render

Right-edge of the footer:

```
v0.4.2  24h <24 sparkline blocks>
```

Sparkline always renders exactly 24 chars. Missing hours render as
`▁` (zero). Range scales by max value in the buffer (with floor of 1
to avoid div-by-zero).

### Startup

`App::new()` loads the last 24 hours of buckets from the store into
`activity_history: VecDeque<(u64, u32)>` (length ≤ 24, sorted by
hour_epoch asc).

## Animation tick

Existing `AppEvent::Tick` already fires every 16ms (see
`app.rs:471`). Add `pub tick: u32` to `App` (saturating-add); increment
on each `Tick`. Spinner frames:

```rust
const SPINNER: [char; 8] = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧'];
fn spinner_frame(tick: u32) -> char { SPINNER[((tick / 8) as usize) % 8] }
```

`tick / 8` divides 60 fps down to ~7.8 fps, matching the spec's 8 fps
spinner cadence.

The dashboard is already re-rendered every tick (60 fps), so the
spinner animates without any extra invalidation logic. The "ago"
column also refreshes on every tick, which is fine — string formatting
of a single u64 per row at 60 Hz is cheap.

## File map

New files:

- `src/ui/dashboard/status.rs` — `Status` enum + `classify(...)` + priority.
- `src/ui/dashboard/spinner.rs` — `SPINNER` table + `frame(tick)` helper.
- `src/ui/dashboard/layout.rs` — top chrome, status strip, footer renderers.
- `src/ui/dashboard/by_repo.rs` — by-repo view (headers + rows + fold).
- `src/ui/dashboard/by_attention.rs` — by-attention view (sections + rows).
- `src/ui/dashboard/row.rs` — shared workspace-row column composer.
- `src/ui/dashboard/sort.rs` — noise score, in-repo priority, fold defaults.
- `src/ui/dashboard/sparkline.rs` — 8-block ramp render.

Modified files:

- `src/ui/dashboard/mod.rs` — thin top-level `render()` that delegates
  to `layout`, then to `by_repo` or `by_attention`. Holds
  `DashboardState { selected, list_state, group_mode, folded, filter }`.
- `src/ui/dashboard/tests.rs` — extend with status mapping, sort,
  fold, sectioning, filter, and snapshot tests using a fixture mirroring
  the design's `data.js`.
- `src/ui/dashboard/label_tests.rs` — port to the new `Status` vocabulary.
- `src/ui/theme.rs` — 6 new status fields, `wsx` theme, `status_style`,
  rename `default_theme` → `ansi`.
- `src/git.rs` — `DiffStats` + `workspace_diff_stats` + parser tests.
- `src/store.rs` — `activity_buckets` table + `set_activity_bucket` /
  `get_activity_buckets` / `prune_activity_buckets_before`.
- `src/app.rs` — drive new `DashboardState` fields; new keybindings
  (`g`, `z`, `r`, `/`); `tick: u32`; `workspace_diff` map population;
  `activity_history` ring + persistence loop; new `draw()` plumbing.

## Test plan

### Pure functions

- `Status::classify` — truth table over all (awaiting, stopped, stalled,
  secs, running, has_prior) combinations.
- `sort::noise_score` — verified against the design's example data
  ordering (`wsx > ui > scp-admin > ssk > backend > api > frontend > scp-api`).
- `sort::in_repo_priority` — verified against in-repo ordering for the
  wsx repo example (stalled `theme-tokens` first, idle last).
- `sort::default_fold` — empty repo folded; all-idle repo folded; mixed
  unfolded.
- `git::parse_shortstat` — five fixture inputs (zero, only +, only −,
  both, malformed).
- `sparkline::render` — flat zero, single spike, monotonic ramp,
  randomized 24-sample.

### Render snapshots

- By-repo view, with the design's `data.js` fixture ported to Rust:
  assert the rendered `TestBackend` contains the spec'd strings at the
  expected columns. One snapshot for unselected, one for selected.
- By-attention view, same fixture: assert `◆ NEEDS ATTENTION`,
  `● WORKING`, etc. appear with correct counts.
- Folded view: assert collapsed repo doesn't emit its workspace rows.
- Filter: substring match on `theme-tokens` returns only that row.

### Integration

- Activity bucket persistence: write 3 buckets, restart `App`, assert
  `activity_history` reloads them.
- Tick: assert spinner frame increments across 8 ticks.
- Selection navigation: 50 keypress sequence covering `↑↓` skip across
  headers and across section boundaries.

## Open questions (deferred)

- Persistent fold state (Store-backed) — not in v1.
- Mouse mode (click headers to fold, click tabs to toggle group) — not
  in v1.
- Filter substring vs. fuzzy match — substring for v1.
- Per-status custom colors via config (`wsx config set
  status_color.thinking '#c0a0ff'`) — not in v1.
