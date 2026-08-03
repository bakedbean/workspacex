# Workspace-updates modal: sort modes

**Date:** 2026-08-03
**Status:** Approved

## Goal

Let the user re-sort the workspaces in the agent-chat-view updates modal by
workspace status or PR status, in addition to the existing default ordering.

## Behavior

### Sort modes

A new `UpdatesSort` enum with three modes, carried in the modal variant
(`Modal::UpdatesPanel { selected, sort }`). The mode is **not persisted** —
each time the modal opens it starts in `Default`.

- **Default** — today's ordering, unchanged: `(attention, failed,
  activity_rank, recency)`.
- **Status** — by workspace status urgency using the existing
  `Status::priority()`: Stalled → Question → Waiting → Thinking → Complete →
  Idle. A workspace in `WorkspaceState::Failed` ranks above Stalled (failure
  is the loudest signal). Ties fall back to the default sort key.
- **PrStatus** — by `BranchLifecycle`, actionable first: Conflicted → Open →
  Draft → Merged → Closed → NoPr → unknown (no lifecycle data). Ties fall
  back to the default sort key.

### Key binding

`o` (order) cycles Default → Status → PrStatus → Default. `o` is unused in
the modal today (`s`/`v` are splits, `l` is attach). The footer hint shows
the live mode: `[o] sort:default`, `[o] sort:status`, `[o] sort:pr`.

### Grouping

Repo headers and per-repo grouping are unchanged in every mode; the chosen
sort reorders workspaces within each repo section only.

### Selection

Cycling the sort keeps the cursor on the same workspace, not the same row
index: the handler captures the selected workspace id, recomputes the order
under the new mode, and re-points `selected` at that id's new position.

## Implementation

- `src/ui/modal/updates_panel.rs`
  - `UpdatesSort` enum (`Default`, `Status`, `PrStatus`) with a `cycle()`
    helper and a short footer label.
  - `ordered_workspaces_for_panel` gains `sort: UpdatesSort`, `statuses`,
    and `lifecycles` parameters. The per-mode key wraps the existing
    `sort_key` as the tie-breaker.
  - Footer string becomes dynamic to include the sort hint.
- `src/app/render.rs` — pass the modal's `sort` plus the `statuses` /
  `lifecycles` maps it already builds.
- `src/app/input.rs` — the `Modal::UpdatesPanel` arm builds `statuses`
  (via `app.classify_status`) and uses `app.pr_lifecycle`, mirroring
  render.rs; adds the `o` handler with id-preserving reselection.
- `src/ui/modal/mod.rs` — extend the `Modal::UpdatesPanel` variant with
  `sort` and re-export `UpdatesSort`.

Renderer and key handler keep sharing the one ordering function so rows and
key indices cannot drift apart.

## Testing

- Unit tests on the ordering function: each mode's ordering, failed ranks
  above Stalled in Status mode, unknown lifecycle ranks last in PrStatus
  mode, ties fall back to the default key.
- Input tests: `o` cycles through the three modes and back; cycling
  preserves the selected workspace id; modal reopens in `Default`.
- Footer hint reflects the active mode.
