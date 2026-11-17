# Focus view per-commit diff bars Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the focus view's solid horizontal diff bar with one vertical bar per commit (`▂▅▁▃█▂▄  +184 −22`).

**Architecture:** The 10s diff poll's existing `git log base..HEAD` gains `--shortstat`, so each `BranchCommit` carries its line counts. The focus view's `WorkspaceDetail` gets the per-commit sizes from `app.workspace_branch_lists`; `tall.rs` draws them in place of `diff_bar`.

**Tech Stack:** Rust, ratatui, tokio, git CLI.

**Spec:** `docs/superpowers/specs/2026-11-17-focus-diff-commit-bars-design.md`

> **Superseded in part:** Task 1 below adds `--shortstat` to the 200-commit
> `branch_commits` log. Review found that diffed up to 200 commits every
> 10s poll, so the shipped code keeps that log diff-free and fetches stats
> with a separate `recent_commit_stats` (`git log -n8 --shortstat`). The
> spec describes the final design; don't restore the version in Task 1.

## Global Constraints

- Up to `DIFF_BARS = 8` bars: the 8 newest commits, oldest on the left.
- Glyphs `▁▂▃▄▅▆▇█` (reuse `sparkline::BLOCKS`); log scale (`ln_1p`) against the largest single commit across the view's rows; a commit with changes gets at least `▁`.
- A commit with no line changes (`stats` `None`, or `0/0`) draws `▁` in `theme.dim_style()`.
- Green (`theme.ok_style()`) when `added >= removed`, else red (`theme.err_style()`).
- `+N −M` totals stay exactly as today; no new git process; other views untouched.
- No Claude attribution in commits (user rule overrides the template).

## Review Focus

- Rows with different commit counts (3 vs 8): `+N −M` must still start in the same column → test in Task 2 (`commit_bars_pad_to_the_widest_row`).
- Branch with a diff but no commit data yet (first poll / log failed): totals with no bars, no panic → test in Task 2 (`diff_without_commits_shows_totals_only`).
- Merge commit in the log (git prints no shortstat for it): must parse as `stats: None`, not steal the next commit's stats → test in Task 1.
- A single 5000-line commit next to 1-line ones: small ones still visible as `▁` → test in Task 2 (`commit_bars_log_scale_keeps_small_commits_visible`).
- Two-line strip layout (narrow terminal): diff cell width still comes from `strip_columns` → covered by existing two-line tests staying green.

---

### Task 1: Per-commit line counts from `git log --shortstat`

**Files:**
- Modify: `src/git/mod.rs:988-1054` (`BranchCommit`, `branch_commits`, `parse_branch_commits`) and its test at `:289`
- Modify (add `stats: None`): `BranchCommit { .. }` literals in `src/ui/dashboard/side_panel.rs`, `src/app/input/tests/side_panel.rs`, `src/app/input/tests/repo_focus.rs`

**Interfaces:**
- Produces: `BranchCommit { hash: String, subject: String, stats: Option<DiffStats> }`. `stats` is `None` when git printed no shortstat line for the commit (merge, empty commit).

- [ ] **Step 1: Write the failing test** in `src/git/mod.rs` tests, replacing `parse_branch_commits_skips_malformed_lines`' input/expectations to include `stats`, plus:

```rust
#[test]
fn parse_branch_commits_pairs_shortstat_lines() {
    let out = "aaa1111\tadd thing\n\n 2 files changed, 10 insertions(+), 3 deletions(-)\n\
               bbb2222\tMerge branch 'x'\n\
               ccc3333\tdrop file\n\n 1 file changed, 4 deletions(-)\n";
    let parsed = parse_branch_commits(out);
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].stats, Some(DiffStats { added: 10, removed: 3 }));
    assert_eq!(parsed[1].stats, None, "merge has no stat line");
    assert_eq!(parsed[2].stats, Some(DiffStats { added: 0, removed: 4 }));
}
```

- [ ] **Step 2: Run** `cargo test --lib git::tests::parse_branch_commits` — Expected: FAIL (no `stats` field).

- [ ] **Step 3: Implement.** Add the field; add `"--shortstat"` to the `run` args in `branch_commits`. In `parse_branch_commits`, a line starting with a space whose `parse_shortstat` returns `Some` sets `stats` on the last pushed commit; a line with a tab (and not starting with a space) starts a commit; blank and other lines are skipped. Add `stats: None` to every other `BranchCommit` literal.

- [ ] **Step 4: Run** `cargo test --lib` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/git/mod.rs src/ui/dashboard/side_panel.rs src/app/input/tests/
git commit -m "feat(git): record each branch commit's line counts"
```

### Task 2: Draw one bar per commit in the focus view

**Files:**
- Modify: `src/ui/dashboard/tall.rs` — `WorkspaceDetail` (`:37`), `StripColumns` (`:270-284`), `strip_columns` (`:290-346`), `DIFF_BAR`/`diff_total`/`diff_spans`/`diff_bar` (`:431-484`), the call at `:610`, test `diff_bar_scales_to_the_largest_diff` (`:2017`)
- Modify: `src/app/render/dashboard.rs:425-439` (fill the new field)

**Interfaces:**
- Consumes: `BranchCommit.stats` (Task 1); `app.workspace_branch_lists: HashMap<WorkspaceId, Arc<BranchLists>>` whose `commits` are newest first.
- Produces:
  - `WorkspaceDetail.commit_sizes: Vec<Option<DiffStats>>` — newest first, at most `DIFF_BARS`.
  - `StripColumns.commit_max: u32` (replaces `diff_max`) — largest single commit's `added + removed` across rows.
  - `StripColumns.bars: usize` — most bars any row draws.
  - `const DIFF_BARS: usize = 8;` (replaces `DIFF_BAR`).
  - `fn commit_bars(sizes: &[Option<DiffStats>], max: u32, theme: &Theme) -> Vec<Span<'static>>` — oldest→newest, one cell per commit.
  - `fn diff_spans(row: &RowInputs, detail: &WorkspaceDetail, max: u32, bars: usize, theme: &Theme) -> Vec<Span<'static>>` (plain args, since `strip_columns` calls it before `StripColumns` exists) — bars right-padded with spaces to `bars` cells, then a space and `+N −M` when `bars > 0`; totals only when this row and all rows have no bars. Empty when the row has no diff.

- [ ] **Step 1: Write the failing tests** in `tall.rs` tests (replace `diff_bar_scales_to_the_largest_diff`):

```rust
fn s(a: u32, r: u32) -> Option<DiffStats> { Some(DiffStats { added: a, removed: r }) }

#[test]
fn commit_bars_draw_oldest_left_and_cap_at_eight() {
    let theme = Theme::wsx();
    // newest first in, oldest first out
    let text = |v: &[Option<DiffStats>], max| commit_bars(v, max, &theme)
        .iter().map(|s| s.content.to_string()).collect::<String>();
    assert_eq!(text(&[s(100, 0), s(1, 0)], 100), "▁█");
    let many: Vec<_> = (1..=12).map(|n| s(n, 0)).collect();
    assert_eq!(display_width(&text(&many[..DIFF_BARS], 12)), DIFF_BARS);
}

#[test]
fn commit_bars_log_scale_keeps_small_commits_visible() {
    let theme = Theme::wsx();
    let spans = commit_bars(&[s(1, 0), s(5000, 0)], 5000, &theme);
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().last(), Some('▁'));
    assert_eq!(text.chars().next(), Some('█'));
}

#[test]
fn commit_bars_color_by_dominant_side_and_dim_empty() {
    let theme = Theme::wsx();
    let styles: Vec<_> = commit_bars(&[None, s(1, 9), s(5, 5)], 10, &theme)
        .iter().flat_map(|sp| sp.content.chars().map(move |_| sp.style)).collect();
    // oldest→newest: 5/5 green, 1/9 red, None dim
    assert_eq!(styles, vec![theme.ok_style(), theme.err_style(), theme.dim_style()]);
}

#[test]
fn commit_bars_pad_to_the_widest_row() {
    // Two TallRows: one with 3 commits, one with 8, both with a diff.
    // Render both via `layout` at a wide width; the column where `+` first
    // appears is the same in both strips.
}

#[test]
fn diff_without_commits_shows_totals_only() {
    // A row with diff Some(+12 −3) and empty commit_sizes, alone in the view:
    // the diff cell text is exactly "+12 −3".
}
```

(The last two use the module's existing `TallRow`/`layout` test helpers; assert on the strip line's text.)

- [ ] **Step 2: Run** `cargo test --lib ui::dashboard::tall` — Expected: FAIL (no `commit_bars`).

- [ ] **Step 3: Implement** in `tall.rs`: add the field, constants and functions above; remove `diff_bar`, `DIFF_BAR`, and `diff_total`. Height index per commit: `((total.ln_1p() / max.max(total).ln_1p()) * 7.0).round()` clamped to `0..=7`, into `sparkline::BLOCKS`. Batch adjacent same-style glyphs into one span as `diff_bar` did. In `strip_columns`, compute `commit_max` and `bars` from `r.detail.commit_sizes` before sizing `diff`. Update the doc comment on `diff_spans` to `▂▅▁▃█▂▄  +184 −22`.

- [ ] **Step 4: Fill the field** in `src/app/render/dashboard.rs:425` inside the focused-repo branch:

```rust
commit_sizes: app.workspace_branch_lists.get(&ws.id)
    .map(|l| l.commits.iter().take(DIFF_BARS).map(|c| c.stats).collect())
    .unwrap_or_default(),
```

(Make `DIFF_BARS` `pub` in `tall.rs`.)

- [ ] **Step 5: Run** `cargo test` and `cargo clippy --all-targets -- -D warnings` — Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/tall.rs src/app/render/dashboard.rs
git commit -m "feat(dashboard): draw one bar per commit in the focus view's diff column"
```

### Task 3: See it in the real app

- [ ] **Step 1:** `cargo build`, then run the dashboard with `WSX_BIN=target/debug/wsx` (see the `demo/stills` sandbox), open a repo focus view with branches of differing commit counts, and confirm bars render, align, and update after a new commit within ~10s.
- [ ] **Step 2:** Update the spec's Plumbing line to say the sizes live on `WorkspaceDetail.commit_sizes` (not `RowInputs`), and commit:

```bash
git add docs/superpowers/specs/2026-11-17-focus-diff-commit-bars-design.md
git commit -m "docs: per-commit sizes ride on the focus view's WorkspaceDetail"
```
