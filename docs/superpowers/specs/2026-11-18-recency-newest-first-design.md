# Recency sort: newest first inside buckets

## Why

Recency mode buckets rows by last activity, then orders each bucket by
branch name. Branch names (`eben/4396-…`) carry no meaning for order, so
long-quiet lists — PR sections, tag and PR hide groups, the main list —
read as arbitrary. The user wants them newest first.

## Goal

In Recency mode, every list orders rows inside a bucket by last activity,
most recent first. One rule everywhere: the shared `order_workspaces`
comparator (`src/ui/dashboard/sort.rs:138`), so renderer and nav index
keep agreeing.

Non-goals: Status mode, the blocked pin, bucket boundaries, section and
group order — all unchanged.

## Approaches

- **Exact recency, no buckets.** Simplest, but rows active right now
  trade places every tick — the reshuffle the buckets exist to stop.
- **Newest first inside every bucket except the <2m bucket (picked).**
  Quiet rows age in lockstep, so their relative order never changes on its
  own; a row only moves when it wakes, and then it jumps to the top bucket
  anyway. The <2m bucket keeps the name tiebreak, since its rows update
  constantly. Same stability as today, meaningful order everywhere else.
- **Creation time as the tiebreak.** Stable, but not recency — a merged
  workspace untouched for weeks would still sit above one used yesterday.

## Design

`order_workspaces` Recency key becomes
`(unpinned, tier, newest-first ago unless tier 0, name)`: in tier 0 the
third key is constant; elsewhere it is `ago_secs` ascending. Never-active
rows (`None`) share one tier, so they fall through to name. Name stays the
final tiebreak so the order is total.

Tests in `sort.rs`: replace `recency_orders_rows_inside_one_tier_by_name_not_age`
with one asserting newest first in an older tier, and add one asserting the
<2m tier still orders by name. Update doc comments on `SortMode::Recency`
and `RECENCY_TIERS`.
