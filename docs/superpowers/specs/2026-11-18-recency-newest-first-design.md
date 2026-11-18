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
  Quiet rows' timestamps don't change, so their relative order never
  changes on its own; a row only moves when it wakes, and then it jumps to the top bucket
  anyway. The <2m bucket keeps the name tiebreak, since its rows update
  constantly. Same stability as today, meaningful order everywhere else.
- **Creation time as the tiebreak.** Stable, but not recency — a merged
  workspace untouched for weeks would still sit above one used yesterday.

## Design

`order_workspaces` Recency key becomes
`(unpinned, tier, Reverse(newest-first stamp), name)`. The stamp is the
row's epoch-ms last activity (`SortRow::sort_last_active_ms`, carried on
`RowInputs.last_active_ms` and `NavRow`), or `None` in tier 0 so live rows
keep name order. It must be the timestamp, not `ago_secs`: two rows active
less than a second apart floor to ages that tie and untie as the clock
ticks, which would flip them between name and age order (the top bar
avoids the same trap, `src/app/render/dashboard.rs:443`). Tier and pin
still come from `ago_secs`; both are monotonic in the timestamp, so they
never contradict it. Never-active rows share one tier and fall through to
name, the final tiebreak that keeps the order total.

Tests in `sort.rs`: tier-0 name order, older tiers newest first, a
sub-second-apart pair held steady across several frame times, exact-stamp
ties by name, pinned rows in one older tier, never-active rows by name.
