# Dashboard footer modules — design

**Date:** 2026-09-15
**Status:** approved, awaiting implementation plan

## Problem

The dashboard footer's right-hand slot draws `$usage`, a 24-bar sparkline of
the maximum number of live agents per hour. It is retrospective; every other
element of the dashboard describes the fleet *now*. The slot is the only
persistent, always-visible strip on the dashboard, so it should carry the
highest-signal at-a-glance summary of the fleet — and which summary that is
depends on how a given user runs wsx.

## Goal

Make the slot a **pluggable, configuration-driven module** in the same spirit
as the workspace detail bar's modules, but authored by users in `theme.toml`
rather than compiled in: a user composes a module from fleet-wide variables
using the existing starship-style bar grammar, and places it in any bar.
Ship one preset, `funnel`, as the new default in the slot.

## Non-goals (v1)

- Click targets on modules. Modules are read-only; `$usage` keeps its own
  picker click. Actions are a follow-up once the variable set has settled.
- External-command modules (waybar/starship `custom`-style `exec`). Nothing
  in the config shape precludes adding a `command` key later.
- A settings-side "which modules are active" list or in-app picker.
  Selection is placement in a bar's `format`/`right_format`.
- New data collection. Every v1 variable is derived from state the app
  already holds in memory, plus one cheap count that is already queried.
- Removing `$usage`. It stays a built-in segment; only the default theme
  stops placing it.

## Decisions (already made)

| Question | Decision |
|---|---|
| What can a user author without recompiling? | Modules composed from built-in **fleet variables** in theme.toml. |
| Where do definitions live? | `theme.toml`, as `[module.<name>]` tables. No new settings key. |
| Clickable? | No, not in v1. |
| Reference syntax | `$<name>` — the module's table name, unqualified. The format grammar is unchanged (`$name` identifiers are `[A-Za-z0-9_]`, `src/ui/bar/format.rs:112`), so `$module.funnel` is not an option without a grammar change; the loader rejects name collisions instead. |
| Zero counts | A count variable renders **empty** when zero, so the grammar's `( … )` conditional groups drop the item. A user who wants an explicit `0` cannot get one in v1. |
| Scope of a module | Global. Fleet stats are the same in every bar, so a module may be placed in `dashboard_footer`, `dashboard_header`, `attached_top`, `attached_bottom`, or `dashboard_detail`. |

## Rejected alternatives

- **Fleet variables directly in bar formats** (`right_format = "($pr_open review)"`, no module tables). Smallest loader change, but a module can't be named, reused across bars, or dropped as a unit under `priority`; loses the "swap modules in and out" ergonomics.
- **Rust registry + config selection** (a clone of `src/ui/detail_modules`). Users could only pick and arrange compiled-in modules, not build their own.

## Config shape

A module is a top-level TOML table under the reserved `module` namespace:

```toml
[module.funnel]
format   = "[$working ⚙](fg:ok) ([$blocked ✋](fg:err) )([$review_required 👀](fg:waiting) )([$changes_requested ✎](fg:warn) )([$mergeable ✓](fg:merged) )"
style    = ""        # optional; forms $style exactly as for a segment
priority = 50        # optional; default 100 (never drops)
disabled = false     # optional

[dashboard_footer]
right_format = "($version  )$funnel"
```

`ModuleTable` accepts exactly `format`, `style`, `priority`, `disabled`
(`deny_unknown_fields`). Modules are not multi-item, so `separator`,
`more_format`, `styles`, and `symbol` are rejected as unknown keys.

Merging follows the segment rule (`ThemeFile::merge_over`,
`src/config/theme_file.rs:122`): the bundled default's `[module.*]` tables
are unioned with the user's, and the user wins per field. A user can
therefore restyle the shipped `funnel` by setting only `format`, or replace
it wholesale.

### Validation (theme errors, surfaced at load and hot-reload)

| Condition | Error location / message |
|---|---|
| `format` references a `$var` not in `FLEET_VARS` (and not `$style`) | `[module.<name>].format` — `unknown $var (fleet variables: …)` |
| module name equals a built-in segment name (`keys`, `usage`, …) | `[module.<name>]` — `name collides with built-in segment` |
| a bar format references `$x` that is neither a segment nor a defined module | existing "unknown segment" error, hint now lists modules too |
| invalid style token | existing style error path |

Modules never carry a hit, so `check_singletons` is unaffected.

## Fleet variables (v1)

Declared in a static table `FLEET_VARS: &[FleetVar]` (`name`, `doc`) in
`src/ui/bar/registry.rs`, beside `SEGMENTS`, so the loader validates module
formats the same way it validates segment formats and the book can be
generated from one list.

Counts are over **every workspace the dashboard lists** (all repos), using
the same in-memory maps the dashboard rows read.

| Variable | Source | Meaning |
|---|---|---|
| `working` `waiting` `blocked` `done` `busy` | `app.pushed_status[ws].state` (`ReportedState`) | workspaces whose last `wsx status set` (or Stop-hook inference, for `busy`) is that state |
| `unreported` | absence of `pushed_status` entry | workspaces with no reported status |
| `attention` | `app.workspace_needs_attention` | unacknowledged attention alerts |
| `awaiting` `stalled` `active` `idle` | `app.workspace_activity[ws]` (`ActivityState`) | live transcript classification; `awaiting` = `AwaitingAnswer` |
| `live_agents` | same predicate `run.rs:290` uses for the sparkline bucket | workspaces whose primary status is `Thinking` or `Waiting` |
| `pr_none` `pr_draft` `pr_open` `pr_conflicted` `pr_merged` `pr_closed` | `app.pr_lifecycle[ws]` (`BranchLifecycle`) | PR lifecycle counts; a workspace never polled counts in none of them |
| `review_required` `changes_requested` `approved` | `app.pr_review[ws]` (`ReviewDecision`) | review verdict counts |
| `unresolved` | `Σ app.pr_unresolved[ws]` | total unresolved review threads across the fleet |
| `mergeable` | derived | `pr_lifecycle == PrOpen && pr_review == Approved` (conflicted is its own lifecycle, so it's excluded by construction) |
| `dirty` | `app.workspace_status[ws]` (`git::WorkspaceStatus`) | `modified + untracked > 0` |
| `msgs_queued` | `store.undelivered_messages().len()` | agent-to-agent messages not yet delivered |
| `workspaces` `repos` | `app.workspaces.len()`, `app.repos.len()` | totals (never empty; `0` renders as `0`) |

Rendering rule: every variable is a decimal integer. All except
`workspaces` and `repos` render empty at zero. No variable carries a colour
of its own; `$style` is the module's `style` patched over the bar style.

"Merged today" from the original pitch is dropped: `scm_cache` has no
`merged_at`. `pr_merged` (merged, not yet archived) stands in.

## Architecture

```
App state ──► FleetStats::collect(&App)   (src/ui/bar/fleet.rs, once per frame)
                    │
                    ▼  to_vars() -> SegmentMap  (variable name -> Segment)
theme.toml ──► BarSpecs.modules: HashMap<String, SegmentConfig>
                    │
                    ▼
bars.rs composers: for each (name, cfg) in specs.modules
        put(segments, name, providers::module(cfg, &fleet_vars, &resolver))
                    │
                    ▼
render_bar(...)  — unchanged: priority drop, groups, fill, right_format
```

### Components

**`src/ui/bar/registry.rs`** — add `pub struct FleetVar { name, doc }` and
`pub const FLEET_VARS`. Add `pub fn fleet_var(name) -> Option<&FleetVar>`.
The doc string is the single source for the book table.

**`src/ui/bar/fleet.rs` (new)** — `pub struct FleetStats { … u32 fields … }`
with `pub fn collect(app: &App, msgs_queued: u32) -> FleetStats` and
`pub fn to_vars(&self) -> SegmentMap`. Pure over `App`'s maps; no I/O.
`msgs_queued` is passed in so `collect` stays I/O-free and testable — the
app reads `undelivered_messages()` on its existing refresh tick (the same
tick that drains the queue) and caches the count on `App`.

**`src/config/theme_file.rs`** — `ThemeFile.module: BTreeMap<String,
ModuleTable>` (`#[serde(default)]`), `ModuleTable::merge_over`,
`resolve_module(name, tbl, resolver, errors) -> Option<SegmentConfig>`
mirroring `resolve_segment` but validating against `FLEET_VARS` and
`placeholder_styles(&["style"])`. `BarSpecs.modules: HashMap<String,
SegmentConfig>`. `resolve()` builds `allowed_names = SEGMENTS ∪ modules`
and passes it to every `resolve_bar` call in place of `segment_names`.
Collision check runs before `resolve_module`.

Because `segments` is a flattened map of *every other* top-level table,
`module` must be a named field declared before the `#[serde(flatten)]`
segments map, otherwise serde would route `[module.x]` into `segments` and
reject it as an unknown segment.

**`src/ui/bar/providers.rs`** — `pub fn module(cfg: &SegmentConfig, vars:
&SegmentMap, resolver: &Resolver) -> Option<Segment>`: `eval_segment(cfg,
vars, Style::default(), &[], resolver)`. No hit.

**`src/ui/bar/bars.rs`** — every composer (`dashboard_footer`,
`dashboard_header`, `attached_top`, `attached_bottom`, `dashboard_detail`)
takes `fleet: &SegmentMap` on its inputs struct and, before `render_bar`,
inserts every module from `specs.modules`. A shared helper
`put_modules(&mut segments, specs, fleet, &resolver)` keeps the five call
sites identical.

**`src/app/render/mod.rs` / `dashboard.rs` / `attached.rs`** — compute
`FleetStats::collect(app, app.msgs_queued).to_vars()` once at the top of
the frame and pass it to each composer's inputs.

**`src/ui/bar/default_theme.toml`** — add `[module.funnel]` (format above,
`priority = 50` so it drops before `$keys` on narrow terminals, matching
today's `$usage` behaviour) and change `[dashboard_footer].right_format` to
`"($version  )$funnel"`. Comment block documents the `[module.*]` grammar
and lists `FLEET_VARS` by name.

**`docs/examples/theme-*.toml`** (7 files) — each currently places
`$usage`; switch to `$funnel` with a palette-matched restyle. Separate
commit; taste-driven, so keep to one colour pass and no layout changes.

**`docs/book/src/configuration/themes.md`** — new `### Modules` section
after `### Segments`: table shape, the fleet-variable table, the zero-is-
empty rule, the collision rule, and a two-line "bring `$usage` back" recipe.

## Data flow per frame

1. Refresh tick (existing) loads `pushed_status`, PR maps, activity map;
   additionally stores `app.msgs_queued = undelivered_messages().len()`.
2. Draw: `FleetStats::collect` walks `app.workspaces` once (O(n), n = a few
   dozen) and builds the `SegmentMap`.
3. Each bar composer evaluates every defined module against that map and
   inserts the result; modules a bar's format doesn't reference cost one
   `eval_segment` and are otherwise ignored, exactly like unreferenced
   segments today.

## Error handling

- Theme errors use the existing `ThemeError` surface (`wsx theme` CLI and
  the in-app reload banner); a failing user theme falls back to the last
  good specs as today. The bundled default must resolve with zero errors —
  covered by the existing `bundled_default` test.
- A workspace missing from any map contributes to no count except
  `unreported` / `workspaces`. No panics on partial state.
- `msgs_queued` read failure logs at `warn` and keeps the previous count.

## Testing

- `registry.rs`: `FLEET_VARS` names are unique, snake_case, and disjoint
  from `SEGMENTS` names and `ITEM_COLORS`.
- `fleet.rs`: fixture `App` with ~6 workspaces exercising every variable;
  `mergeable` derivation (open+approved counts, open+changes_requested
  doesn't, merged+approved doesn't); zero renders empty; `workspaces`/
  `repos` render `0`.
- `theme_file.rs`: `[module.x]` with unknown `$var` errors; `[module.keys]`
  collision errors; `[module.x]` with `separator` is an unknown-field
  error; module referenced from `attached_bottom` resolves; user table
  overriding only `format` keeps the default's `priority`; a bar
  referencing an undefined `$module_name` errors with the combined hint.
- `bars.rs`: default footer renders `$funnel` and not the sparkline; a
  module with `priority = 50` drops before `$keys` when width is short;
  `$usage` still renders when a theme places it.
- Existing bar snapshot/render tests updated for the new default
  `right_format`.

## Commit plan

1. `FLEET_VARS` + `FleetStats` (registry, fleet.rs, App field for
   `msgs_queued`), with tests.
2. Theme loader: `[module.*]` tables, validation, `BarSpecs.modules`.
3. `providers::module` + composer wiring + per-frame collection.
4. Default theme: `[module.funnel]` preset, footer `right_format`, test
   updates.
5. Example themes restyled to `$funnel`.
6. Book: `### Modules` section.

## Follow-ups (not in scope)

- `bottleneck` preset: needs age variables (`oldest_blocked_age`,
  `oldest_awaiting_age`) and a name variable — strings, not counts, so the
  zero-is-empty rule needs a companion "absent-is-empty" rule.
- `mesh` preset: `msgs_delivered_recent`, from→to pairs.
- Click actions on modules (`on_click = "filter:pr_open"` or variable-bound
  clicks), which also needs a dashboard filter-by-predicate primitive.
- `exec` modules.
