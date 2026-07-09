# Colorize the H shared-workspace picker by GitHub PR status

**Date:** 2026-07-08
**Status:** Approved

## Problem

The dashboard colors each workspace's branch by its GitHub PR lifecycle (open /
conflicted / merged / closed) and shows a matching PR glyph. The `H` keybind's
remote shared-workspace picker (`Modal::RemoteWorkspaceList`) shows the same
kind of rows — repo / workspace / branch / agent — but renders them in a single
flat style with no PR information. We want the picker to carry the same visual
vocabulary as the dashboard.

## Constraint that shapes the design

The picker lists workspaces on a **remote** host, fetched over SSH via
`ssh <host> 'wsx shared list --json'` (`commands::shared_hosts::fetch_shared_list`).
The local `App.pr_lifecycle` cache is keyed by *local* `WorkspaceId` and is
populated by running `gh` in *local* worktrees, so it has nothing for the remote
host's workspaces. A PR's lifecycle is a property of the branch on the shared
forge, though, so either side can compute it. **Decision: the remote computes
it** inside `wsx shared list --json`, so the exact same `BranchLifecycle` value
flows through the exact same `Theme::lifecycle_style` mapping the dashboard uses
— they cannot drift.

## Decisions

- **Visual treatment:** full dashboard parity — colored branch **and** PR glyph.
- **Failure/latency:** best-effort, concurrent. Opening `H` runs `gh pr view`
  per shared workspace on the remote; results are fetched concurrently, and any
  failure (gh missing, unauthenticated, network error) leaves that row
  uncolored. The list always renders.

## Data flow

```
remote host:  wsx shared list --json
                └─ shared_list_records()            [pure DB, unchanged, lifecycle: None]
                └─ enrich each record via gh pr view [new, in cli.rs async handler, concurrent]
                     → SharedWorkspaceRecord { …, lifecycle: Option<BranchLifecycle> }
        ↓ ssh (serde JSON over the wire)
local host:   fetch_shared_list() → parse_shared_list_output()  [unchanged; serde picks up new field]
                └─ RemoteList.records
                └─ remote_rows() → RemoteRow { …, lifecycle }   [copy per-ws lifecycle onto each agent row]
                └─ render_remote_workspace_list(…, nerd_fonts)  [multi-span: colored branch + PR glyph]
```

## Changes

### 1. Make `BranchLifecycle` serializable — `src/git/forge.rs`
Add `serde::Serialize, serde::Deserialize` to its derives. Plain unit enum →
serializes as variant-name strings.

### 2. Add the wire field — `src/commands/shared.rs`
`SharedWorkspaceRecord` gains `#[serde(default)] pub lifecycle:
Option<BranchLifecycle>`. `#[serde(default)]` keeps the wire contract
additive-only (the module's stated invariant): a remote on an older wsx that
does not emit the field deserializes as `None`. `shared_list_records` stays a
pure, sync, network-free DB function and fills `lifecycle: None`.

### 3. Enrich on the remote — `src/cli.rs` (`CliAction::SharedList`, already async)
After `shared_list_records` builds the records, run
`forge::fetch_pr_status(worktree_path, branch)` for every record concurrently
(`futures::future::join_all` or equivalent), mapping `Ok(Some(s)) →
Some(s.lifecycle)` and every other outcome → `None` (best-effort). Populate each
record's `lifecycle`. `fetch_pr_status` already degrades to `Ok(None)` when `gh`
is unusable, so failures are naturally best-effort.

### 4. Plumb lifecycle onto the row — `src/app.rs`
`RemoteRow` gains `lifecycle: Option<BranchLifecycle>`; `remote_rows` copies
`rec.lifecycle` onto each flattened agent row.

### 5. Render with dashboard parity — `src/ui/modal/remote_workspace_list.rs` + `src/app/render.rs`
- Thread `nerd_fonts` into the renderer; the call site computes
  `nerd_fonts_enabled(&app.store)` — a *local* display preference, correctly
  resolved locally.
- Split each row from one span into several: `repo/workspace` (default), **PR
  glyph + branch** colored via `theme.lifecycle_style(row.lifecycle)
  .unwrap_or_else(|| theme.dim_style())`, `label`, liveness marker.
- Selected row applies `selected_bg_style()` (background-only tint) instead of
  the full `selected_style`, so the lifecycle foreground survives selection —
  the same pattern the dashboard list uses.
- **Extract the glyph map:** move the inline `lifecycle → branch glyph` match
  (currently `dashboard/row.rs`) into one shared `pub(crate) fn
  branch_glyph(lifecycle, nerd_fonts)` in `theme.rs` (alongside
  `lifecycle_style`), called by both the dashboard and the picker so they can't
  drift.

## Testing

- **forge:** serde round-trip for `BranchLifecycle` (all variants).
- **shared record:** round-trip `SharedWorkspaceRecord` with `lifecycle:
  Some(...)`; assert legacy JSON with no `lifecycle` key deserializes to `None`.
- **remote_rows:** a record with `lifecycle: Some(PrOpen)` yields agent rows
  each carrying that lifecycle.
- **renderer:** extend the existing `TestBackend` tests to assert the branch
  cell carries the expected color per lifecycle in **both** nerd-font modes
  (mirrors the badge-color test from #230), plus a `None`-lifecycle row renders
  uncolored.
- **glyph helper:** unit-test `branch_glyph` for each variant × nerd/plain.

## Out of scope (YAGNI)

Local caching of remote PR status, a timeout knob (best-effort covers it),
coloring the host picker (stage A of `H`), and CI/check-run status beyond PR
lifecycle (the dashboard does not do that either).

## Commits

1. `feat(forge): make BranchLifecycle serde + extract branch_glyph helper`
2. `feat(shared): carry PR lifecycle in shared list records, computed on the remote`
3. `feat(shared): colorize H picker branch by PR status`
