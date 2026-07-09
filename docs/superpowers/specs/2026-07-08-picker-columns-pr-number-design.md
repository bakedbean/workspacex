# H picker: 3-column layout + PR number

**Date:** 2026-07-08
**Status:** Approved

## Problem

The `H` remote shared-workspace picker (`Modal::RemoteWorkspaceList`) renders each
row as `  repo/workspace  <glyph> branch  agent  ●` with variable-width, double-space
separators, so nothing lines up vertically. We want fixed, aligned columns in a
new order, and — since we already hit GitHub for PR status — to surface the PR
number next to the branch.

## Target layout

Three left-aligned, fixed-width columns:

```
 <agent>    <glyph> #<num> <branch>                    <repo>/<workspace>
 claude     ⎇ #2087 eben/billing-v2-migration-rollo…   ssk-web/mighty-azalea
 codex#2    ⎇ #2091 eben/quickbooks-invoice-sync        ssk-web/clever-juniper
```

- **Col 1 — agent**: full instance label (`claude`, `codex#2`), neutral color.
  Full label (not just kind) so multiple instances on one workspace — which the
  picker flattens into separate rows — stay distinguishable.
- **Col 2 — branch**: `<glyph> #<num> <branch>`, colored by PR lifecycle exactly
  as today (via `theme::lifecycle_style`, dim when no PR / unknown). `#<num>`
  appears only when a PR number exists; no-PR rows render `<glyph> <branch>`.
- **Col 3 — repo/workspace**: `ssk-web/mighty-azalea`, neutral. Keeps the repo
  prefix so codenames stay unambiguous when a host shares workspaces across repos.

The trailing `●`/`✗` liveness marker is **removed**: the picker is attach-only
(`remote_rows` filters to alive rows), so it was always `●` and carried no
information.

## Alignment mechanism

Compute each column's width as the widest cell across all rows (capped), then pad
every cell to that width so columns align regardless of content. When the panel is
narrow (its width is clamped to `[20,100]`), truncation priority is:

1. **branch** first — longest; truncates the branch *name* while always preserving
   the leading `<glyph> #<num> `.
2. **repo/workspace** next.
3. **agent** kept intact (shortest, and the primary row identifier).

Reuse the dashboard's `truncate_pad` helper (in `ui/dashboard/row.rs`) so
truncation/padding behavior matches the rest of the app; promote it to a shared
location if it isn't already reachable from `ui/modal`.

## Data plumbing (the PR number)

`fetch_pr_status` already returns `PrStatus { lifecycle, number }`, but
`enrich_with_pr_status` currently keeps only `lifecycle`. Mirror the existing
`lifecycle` flow:

1. `SharedWorkspaceRecord` gains `#[serde(default)] pub pr_number: Option<u32>` —
   additive wire field, same pattern/justification as `lifecycle`.
2. `enrich_with_pr_status` populates both `lifecycle` and `pr_number` from the
   fetched `PrStatus`.
3. `RemoteRow` gains `pr_number: Option<u32>`; `remote_rows` copies it from the
   record onto each flattened agent row.
4. The renderer draws `#<num>` in the branch cell when present.

## Rendering details

- Selection still uses `selected_bg_style()` (background-only tint) so the
  lifecycle foreground survives selection.
- `#<num>` shares the branch cell's lifecycle color (it is part of the PR-status
  cell).
- Column order in the built spans: agent, branch, repo/workspace.

## Testing

- **renderer**: extend the `TestBackend` tests to assert (a) the three columns
  start at the same x across multiple rows (alignment), (b) `#<num>` renders when
  a PR number is present and is absent for a `None` number, (c) the branch cell
  still carries the expected lifecycle color, (d) the liveness marker no longer
  renders.
- **serde**: round-trip `SharedWorkspaceRecord` with `pr_number: Some(..)`; assert
  legacy JSON without the key decodes to `None`.
- **enrich**: existing best-effort test still holds; extend to assert `pr_number`
  is `None` on the degraded path.
- **remote_rows**: a record with `pr_number: Some(..)` yields rows carrying it.

## Out of scope (YAGNI)

Agent-cell coloring by agent identity (dashboard parity — easy follow-up, not
requested), a column header row, and coloring the repo/workspace cell.

## Commits

1. `feat(shared): carry PR number in shared list records`
2. `feat(shared): realign H picker into agent/branch/workspace columns with PR number`
