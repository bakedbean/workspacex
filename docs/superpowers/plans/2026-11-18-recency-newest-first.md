# Recency Newest-First Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Recency mode, order rows inside each recency tier newest first, except the <2m tier, which keeps the name order.

**Architecture:** One change to the Recency key in `order_workspaces`; every list (main, PR sections, hide groups, nav index) already sorts through it.

**Tech Stack:** Rust, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-11-18-recency-newest-first-design.md`

## Global Constraints

- Status mode, pin rules, `RECENCY_TIERS` bounds unchanged.
- Name remains the final tiebreak (order must be total).

## Review Focus

- Never-active rows (`None`) still sort last, by name among themselves — existing `recency_sorts_never_active_rows_last` plus new test.
- Pinned rows among themselves: older-tier pinned rows order newest first — covered by existing `recency_orders_pinned_rows_among_themselves_by_recency`.

---

### Task 1: Newest-first tiebreak in `order_workspaces`

**Files:**
- Modify: `src/ui/dashboard/sort.rs:42-47,86-96,138-160` and its tests

- [ ] **Step 1: Tests.** Replace `recency_orders_rows_inside_one_tier_by_name_not_age` with:
  - `recency_keeps_name_order_inside_the_active_tier`: `[row("zebra", 5), row("alpha", 100)]` → `["alpha", "zebra"]`.
  - `recency_orders_older_tiers_newest_first`: `[row("a-old", 20*HOUR), row("z-new", 7*HOUR)]` (both in the 6–24h tier) → `["z-new", "a-old"]`; and `[row("a", 40*24*HOUR), row("b", 2*24*HOUR)]` → `["b", "a"]`.
  - `recency_orders_never_active_rows_by_name`: two `ago_secs: None` rows `"b"`, `"a"` → `["a", "b"]`.
- [ ] **Step 2:** `cargo test --lib ui::dashboard::sort` — the older-tier test fails.
- [ ] **Step 3:** Recency key becomes `(unpinned, tier, age_key, name)` where `age_key` is `0` for tier 0 and `ago_secs.unwrap_or(0)` otherwise. Update the `SortMode::Recency` and `RECENCY_TIERS` doc comments.
- [ ] **Step 4:** `cargo test` and `cargo clippy --all-targets -- -D warnings` pass.
- [ ] **Step 5:** Commit `feat(dashboard): order recency tiers newest first`.
