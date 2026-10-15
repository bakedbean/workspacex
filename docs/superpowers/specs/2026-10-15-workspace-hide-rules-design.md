# Workspace hide rules — design

**Date:** 2026-10-15
**Status:** implemented

## Problem

Long-lived workspaces pile up in some repos. Once a PR merges, or a branch
never gets a PR, the row keeps its slot on the dashboard and crowds out the
work in flight. The `/` filter can't help: it matches text only and clears on
`Esc`.

## Goals

- Per-repo, persistent rules that hide workspaces from the dashboard.
- First criterion: PR state (no PR, draft, open, conflicted, merged, closed).
- The rule model must admit future criteria (idle time, agent status) without
  a storage or UI rewrite.
- Hidden workspaces stay discoverable: each repo shows one summary row per
  rule that is hiding something, with the rule's label and a count.

## Non-goals

- CLI to set rules (`wsx repo set --hide …`). Follow-up.
- Non-PR criteria. The model allows them; none ship here.
- Global/default rules across repos.
- Changing fleet, status-bar, waybar, or menubar counts. Hiding is a
  dashboard declutter only.

## Rule model

New module `src/data/hide_rules.rs` (rule model; glyph/style come from the dashboard via `HideRule::lifecycle()`):

```rust
pub enum HideRule {
    Pr(PrCriterion),
}

pub enum PrCriterion { NoPr, Draft, Open, Conflicted, Merged, Closed }

impl HideRule {
    pub fn matches(&self, w: &WorkspaceItem<'_>) -> bool;
    pub fn label(&self) -> &'static str;      // "merged", "no PR", …
    pub fn glyph(&self, theme: &Theme) -> …;   // reuses lifecycle glyph/style
    pub fn token(&self) -> &'static str;      // "pr:merged", "pr:none", …
    pub fn from_token(s: &str) -> Option<Self>;
}

pub struct HideRules(Vec<HideRule>);  // ordered, deduplicated

impl HideRules {
    pub fn parse(raw: &str) -> Self;      // comma list; unknown tokens skipped
    pub fn serialize(&self) -> String;
    /// First rule matching `w`, or None. First-match assignment keeps counts
    /// disjoint once overlapping criteria exist.
    pub fn classify(&self, w: &WorkspaceItem<'_>) -> Option<&HideRule>;
}
```

- `Pr(_)` matches only when the row's `lifecycle` is `Some(state)` and equals
  the criterion. `None` (PR lookup not yet resolved) never matches, so rows
  don't blink out and back in on cold start.
- Tokens are namespaced (`pr:merged`, `pr:none`, `pr:draft`, `pr:open`,
  `pr:conflicted`, `pr:closed`) so a future `idle:14d` can't collide.
- Unknown tokens are skipped on parse, so an older binary doesn't fail on a
  newer database. They are not preserved: the picker rewrites the full list.
- Rule order is the canonical order of the picker list, not toggle order, so
  summary rows render in a stable sequence.

## Storage

- New nullable column `repos.hide_rules TEXT`, added in `src/data/schema.rs`
  with the existing "check column, then `ALTER TABLE`" migration pattern.
  No seed: `NULL` means no rules.
- `Repo` gains `hide_rules: Option<String>` (raw). Parsing happens at the
  dashboard boundary, matching how `detail_bar_config` is carried.
- Store setter `set_repo_hide_rules(repo_id, Option<&str>)`; empty list
  writes `NULL`.

## Visibility

A single predicate sits beside `matches_filter` in
`src/ui/dashboard/mod.rs`:

```rust
fn hidden_by(w, rules, state) -> Option<&HideRule>
```

It returns `None` (row visible) when either:

- a `/` filter is active and non-empty (searching bypasses hide rules), or
- the row's group is expanded for this session (see below).

Otherwise it returns `rules.classify(w)`.

Every path that calls `matches_filter` today also consults it:
`render_by_repo`, `render_by_attention`, `visible_targets`,
`quiet_repo_names`, and the updates panel
(`src/ui/modal/updates_panel.rs`).

## Summary rows

Under each repo, after its visible workspace rows (independent of sort
mode), one dimmed row per rule with a nonzero count:

```
▾ workspacex  ~/workspacex ─────────── ●3 ◐1
    feat/foo        working   …
    fix/bar         waiting   …
  ▸ ⏺ merged        4 hidden
  ▸ ○ no PR         2 hidden
```

- Repo header status counts reflect visible rows only.
- A repo whose workspaces are all hidden keeps its header and summary rows.
  It is not "quiet" and never moves to QUIET REPOS.
- A folded repo hides its summary rows along with its workspaces.
- **By-attention view:** summary rows render in a trailing section, grouped
  under each repo name, above QUIET REPOS.
- **Updates panel:** applies hiding but draws no summary rows, to stay
  compact.

### Expanding a group

- `SelectionTarget` gains `HiddenGroup { repo: RepoId, rule: HideRule }`.
- `Enter` / `l` on a summary row expands it: its workspaces render inline
  beneath it, dimmed, and become selectable. The row's glyph flips `▸`→`▾`.
- `h` on an expanded summary row, or on any workspace inside it, collapses
  the group.
- Expansion lives in `DashboardState.expanded_hidden: HashSet<(RepoId,
  token)>`. Session-only, not persisted.
- When a selected workspace becomes hidden (PR merges, rule toggled on), the
  selection moves to the workspace's group summary row; a `HiddenGroup`
  target whose group vanishes falls back the same way a vanished repo does.

## Editing rules: the `F` picker

- `F` on a repo header, a workspace, or a summary row opens
  `Modal::HideRules { repo_id, selected }` for that repo.
- Lists the six PR criteria with `[x]`/`[ ]` and the live count each would
  hide.
- `j`/`k` move, `space` toggles and saves immediately, `esc`/`enter`
  close.
- Saving refreshes the repo row in `App` so the dashboard redraws without a
  reload.

## Docs

- `docs/book/src/daily-use/keybindings.md`: rows for `F`, and `Enter`/`l`/`h`
  on summary rows.
- New short section, "Hiding workspaces", in the daily-use book covering
  rules, summary rows, expanding, and the `/` bypass.

## Testing

- `hide_rules.rs` unit tests: token round-trip, unknown-token skip, dedupe,
  canonical ordering, `None` lifecycle never matches, first-match
  classification.
- Schema test: migration adds the column idempotently across reopen.
- Dashboard render tests (`src/ui/dashboard/tests.rs`): hidden rows absent,
  one summary row per nonzero rule with the right count, visible-only header
  counts, all-hidden repo stays out of QUIET REPOS, `/` bypass, expanded
  group renders dimmed rows.
- `visible_targets` tests: summary rows are selectable; expanded rows are
  selectable; selection parks when a row becomes hidden.
- Input tests: `F` opens the picker for the right repo; `space` persists;
  `Enter`/`h` expand and collapse a group.
- Updates panel test: hidden rows excluded, no summary rows.

## Suggested commit breakdown

1. Rule model plus parsing (`hide_rules.rs`) with unit tests.
2. Schema column, `Repo` field, store setter.
3. Visibility predicate wired into views and `visible_targets` (hiding only).
4. Summary rows, `HiddenGroup` selection, expand/collapse.
5. `F` picker modal.
6. Updates-panel hiding.
7. Docs.
