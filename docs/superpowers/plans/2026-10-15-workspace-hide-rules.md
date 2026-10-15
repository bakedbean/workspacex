# Workspace Hide Rules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Per-repo, persistent rules that hide dashboard workspaces by PR state, with one expandable summary row per active rule so hidden work stays discoverable.

**Architecture:** A UI-free rule model (`src/data/hide_rules.rs`) parsed onto `Repo` at load time. The dashboard asks one predicate (`hidden_by`) which rule hides a row; by-repo and by-attention views render hidden rows as per-rule summary rows that expand inline, selectable through a new `SelectionTarget::HiddenGroup`. An `F` checklist modal edits a repo's rules.

**Tech Stack:** Rust, ratatui, rusqlite, tokio tests.

**Spec:** `docs/superpowers/specs/2026-10-15-workspace-hide-rules-design.md`

## Global Constraints

- Storage tokens are namespaced: `pr:none`, `pr:draft`, `pr:open`, `pr:conflicted`, `pr:merged`, `pr:closed`. Unknown tokens are skipped on parse.
- A row whose PR lifecycle is `None` (lookup not resolved) is never hidden.
- An active, non-empty `/` filter bypasses every hide rule.
- Fleet, status bar, waybar, and menubar counts are unchanged.
- Expanded-group state is session-only; rules persist in `repos.hide_rules`.
- Comments: terse, 2–4 lines, match surrounding density. No Claude attribution in commits.
- Verify each task with `cargo test` (full suite) and `cargo clippy --all-targets -- -D warnings` before committing.

## Review Focus

1. **A PR merges under the selected row** (or the user enables the rule it matches): selection should land on that group's summary row, not vanish into an unhighlighted park. → Task 5, `selected_workspace_falls_back_to_its_group_row_when_hidden`.
2. **Automation jumps to a hidden workspace** (`wsx --select`, waybar): the user expects to land on it. → Task 5, `select_by_name_reveals_hidden_group`.
3. **Every workspace in a repo is hidden:** by-repo must keep the header expanded with its summary rows; by-attention must not list the repo under QUIET REPOS. → Tasks 3 and 4, `all_hidden_repo_keeps_header_and_summary_rows`, `all_hidden_repo_is_not_quiet`.
4. **Rule toggled off while its group is expanded, then on again later:** the group should come back collapsed. → Task 6, `toggling_rule_off_forgets_expansion`.
5. **Cold start before PR polling finishes:** rows must not vanish and reappear. → Task 1, `none_lifecycle_never_matches`, and Task 3, `unknown_lifecycle_row_stays_visible`.

---

## File Structure

- Create `src/data/hide_rules.rs` — `PrCriterion`, `HideRule`, `HideSubject`, `HideRules`. Pure, no UI imports beyond `BranchLifecycle`.
- Modify `src/data/mod.rs` — `pub mod hide_rules;`.
- Modify `src/data/store.rs` — `Repo.hide_rules`, round-trip test.
- Modify `src/data/repo.rs` — select the column, `set_repo_hide_rules`.
- Modify `src/data/schema.rs` — v29 migration.
- Create `src/ui/dashboard/hidden.rs` — `HiddenGroupView`, `hidden_group_line`, `dimmed`. Shared by both views.
- Modify `src/ui/dashboard/mod.rs` — `expanded_hidden`, `hidden_by`, `hidden_groups`, `default_expanded`, both render paths, `visible_targets`, `quiet_repo_names`.
- Modify `src/ui/dashboard/by_repo.rs` — `RepoView.hidden`, render summary rows.
- Modify `src/ui/dashboard/by_attention.rs` — `AttentionData.hidden`, HIDDEN section.
- Modify `src/app/types.rs` — `SelectionTarget::HiddenGroup`.
- Modify `src/app/selection.rs` — `selected_repo`, `hide_subject`, `hidden_group_of`, `selection_target_exists`, `select_workspace_by_name`.
- Modify `src/app/render/dashboard.rs` — fallback-to-group before reconcile.
- Modify `src/app/input/dashboard.rs` — `Enter`/`l`/`h` on groups, `F`.
- Modify `src/ui/modal/mod.rs`, `src/app/input/modal/mod.rs`, `src/app/input/modal/pickers.rs` — `Modal::HideRules`.
- Modify `src/ui/modal/updates_panel.rs` — skip hidden rows.
- Create `src/app/input/tests/hide_rules.rs`; register in `src/app/input/tests/mod.rs`.
- Docs: `docs/book/src/daily-use/keybindings.md`, new `docs/book/src/daily-use/hiding-workspaces.md`, `docs/book/src/SUMMARY.md`.

---

### Task 1: Rule model

**Files:**
- Create: `src/data/hide_rules.rs`
- Modify: `src/data/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum PrCriterion { Merged, Closed, NoPr, Draft, Open, Conflicted }` (`Copy, Eq, Hash, Ord`; declaration order is canonical order)
  - `pub enum HideRule { Pr(PrCriterion) }` (`Copy, Eq, Hash, Ord`)
  - `HideRule::ALL: [HideRule; 6]`, `fn token(self) -> &'static str`, `fn from_token(&str) -> Option<HideRule>`, `fn label(self) -> &'static str`, `fn lifecycle(self) -> Option<BranchLifecycle>`, `fn matches(self, &HideSubject) -> bool`
  - `pub struct HideSubject { pub lifecycle: Option<BranchLifecycle> }` (`Copy, Default`)
  - `pub struct HideRules(Vec<HideRule>)` (`Clone, Debug, Default, PartialEq, Eq`) with `parse(Option<&str>) -> Self`, `serialize(&self) -> Option<String>`, `is_empty`, `contains(HideRule)`, `toggle(&mut self, HideRule)`, `iter() -> impl Iterator<Item = HideRule> + '_`, `classify(&self, &HideSubject) -> Option<HideRule>`

- [ ] **Step 1: Write the failing tests**

Create `src/data/hide_rules.rs` with only the test module, and add `pub mod hide_rules;` to `src/data/mod.rs` after `pub mod agents;`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::forge::BranchLifecycle;

    fn subject(lc: Option<BranchLifecycle>) -> HideSubject {
        HideSubject { lifecycle: lc }
    }

    #[test]
    fn tokens_round_trip() {
        for rule in HideRule::ALL {
            assert_eq!(HideRule::from_token(rule.token()), Some(rule));
        }
    }

    #[test]
    fn parse_skips_unknown_dedupes_and_orders_canonically() {
        let rules = HideRules::parse(Some(" pr:none, idle:14d ,pr:merged,pr:none"));
        assert_eq!(
            rules.iter().collect::<Vec<_>>(),
            vec![HideRule::Pr(PrCriterion::Merged), HideRule::Pr(PrCriterion::NoPr)]
        );
        assert_eq!(rules.serialize().as_deref(), Some("pr:merged,pr:none"));
    }

    #[test]
    fn empty_and_null_parse_to_no_rules_and_serialize_to_none() {
        assert!(HideRules::parse(None).is_empty());
        assert!(HideRules::parse(Some("")).is_empty());
        assert_eq!(HideRules::default().serialize(), None);
    }

    #[test]
    fn toggle_adds_then_removes_and_keeps_order() {
        let mut rules = HideRules::default();
        rules.toggle(HideRule::Pr(PrCriterion::NoPr));
        rules.toggle(HideRule::Pr(PrCriterion::Merged));
        assert_eq!(rules.serialize().as_deref(), Some("pr:merged,pr:none"));
        rules.toggle(HideRule::Pr(PrCriterion::Merged));
        assert_eq!(rules.serialize().as_deref(), Some("pr:none"));
    }

    #[test]
    fn pr_rule_matches_only_its_lifecycle() {
        let merged = HideRule::Pr(PrCriterion::Merged);
        assert!(merged.matches(&subject(Some(BranchLifecycle::PrMerged))));
        assert!(!merged.matches(&subject(Some(BranchLifecycle::PrOpen))));
        let none = HideRule::Pr(PrCriterion::NoPr);
        assert!(none.matches(&subject(Some(BranchLifecycle::NoPr))));
    }

    #[test]
    fn none_lifecycle_never_matches() {
        for rule in HideRule::ALL {
            assert!(!rule.matches(&subject(None)), "{rule:?}");
        }
    }

    #[test]
    fn classify_returns_first_matching_rule() {
        let rules = HideRules::parse(Some("pr:merged,pr:none"));
        assert_eq!(
            rules.classify(&subject(Some(BranchLifecycle::PrMerged))),
            Some(HideRule::Pr(PrCriterion::Merged))
        );
        assert_eq!(rules.classify(&subject(Some(BranchLifecycle::PrOpen))), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib data::hide_rules`
Expected: compile errors — `HideRule`, `HideRules`, `PrCriterion`, `HideSubject` not found.

- [ ] **Step 3: Implement**

Prepend to `src/data/hide_rules.rs`:

```rust
//! Per-repo rules that hide dashboard workspaces. Persisted in
//! `repos.hide_rules` as namespaced tokens (`pr:merged,pr:none`) so future
//! criteria (`idle:14d`) can't collide; unknown tokens are skipped.

use crate::git::forge::BranchLifecycle;

/// A PR-lifecycle criterion. Declaration order is the canonical display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PrCriterion {
    Merged,
    Closed,
    NoPr,
    Draft,
    Open,
    Conflicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HideRule {
    Pr(PrCriterion),
}

/// The facts a rule can test about a workspace. Grows a field per criterion family.
#[derive(Debug, Clone, Copy, Default)]
pub struct HideSubject {
    pub lifecycle: Option<BranchLifecycle>,
}

impl HideRule {
    pub const ALL: [HideRule; 6] = [
        HideRule::Pr(PrCriterion::Merged),
        HideRule::Pr(PrCriterion::Closed),
        HideRule::Pr(PrCriterion::NoPr),
        HideRule::Pr(PrCriterion::Draft),
        HideRule::Pr(PrCriterion::Open),
        HideRule::Pr(PrCriterion::Conflicted),
    ];

    pub fn token(self) -> &'static str {
        match self {
            HideRule::Pr(PrCriterion::Merged) => "pr:merged",
            HideRule::Pr(PrCriterion::Closed) => "pr:closed",
            HideRule::Pr(PrCriterion::NoPr) => "pr:none",
            HideRule::Pr(PrCriterion::Draft) => "pr:draft",
            HideRule::Pr(PrCriterion::Open) => "pr:open",
            HideRule::Pr(PrCriterion::Conflicted) => "pr:conflicted",
        }
    }

    pub fn from_token(s: &str) -> Option<HideRule> {
        HideRule::ALL.into_iter().find(|r| r.token() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            HideRule::Pr(PrCriterion::Merged) => "merged",
            HideRule::Pr(PrCriterion::Closed) => "closed",
            HideRule::Pr(PrCriterion::NoPr) => "no PR",
            HideRule::Pr(PrCriterion::Draft) => "draft",
            HideRule::Pr(PrCriterion::Open) => "open",
            HideRule::Pr(PrCriterion::Conflicted) => "conflicted",
        }
    }

    /// The lifecycle a PR rule stands for, for glyph and color. `None` for
    /// non-PR rules once they exist.
    pub fn lifecycle(self) -> Option<BranchLifecycle> {
        let HideRule::Pr(c) = self;
        Some(match c {
            PrCriterion::Merged => BranchLifecycle::PrMerged,
            PrCriterion::Closed => BranchLifecycle::PrClosed,
            PrCriterion::NoPr => BranchLifecycle::NoPr,
            PrCriterion::Draft => BranchLifecycle::PrDraft,
            PrCriterion::Open => BranchLifecycle::PrOpen,
            PrCriterion::Conflicted => BranchLifecycle::PrConflicted,
        })
    }

    /// An unresolved lifecycle never matches, so rows don't blink out and
    /// back in while PR polling warms up.
    pub fn matches(self, s: &HideSubject) -> bool {
        match self {
            HideRule::Pr(_) => s.lifecycle.is_some() && s.lifecycle == self.lifecycle(),
        }
    }
}

/// A repo's rule set: deduplicated, in canonical order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HideRules(Vec<HideRule>);

impl HideRules {
    pub fn parse(raw: Option<&str>) -> Self {
        let mut rules: Vec<HideRule> = raw
            .unwrap_or("")
            .split(',')
            .filter_map(|t| HideRule::from_token(t.trim()))
            .collect();
        rules.sort();
        rules.dedup();
        HideRules(rules)
    }

    /// `None` when empty, so the column stores NULL rather than "".
    pub fn serialize(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        Some(self.0.iter().map(|r| r.token()).collect::<Vec<_>>().join(","))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, rule: HideRule) -> bool {
        self.0.contains(&rule)
    }

    pub fn toggle(&mut self, rule: HideRule) {
        if let Some(i) = self.0.iter().position(|r| *r == rule) {
            self.0.remove(i);
        } else {
            self.0.push(rule);
            self.0.sort();
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = HideRule> + '_ {
        self.0.iter().copied()
    }

    /// The first rule hiding `s`. First-match keeps group counts disjoint
    /// once overlapping criteria exist.
    pub fn classify(&self, s: &HideSubject) -> Option<HideRule> {
        self.iter().find(|r| r.matches(s))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib data::hide_rules`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/data/hide_rules.rs src/data/mod.rs
git commit -m "feat(data): add per-repo workspace hide rule model"
```

---

### Task 2: Persist rules on repos

**Files:**
- Modify: `src/data/schema.rs` (after the `v < 28` block, ~line 222)
- Modify: `src/data/store.rs:116-130` (`Repo`), tests near `detail_bar_config_column_round_trips` (~line 673)
- Modify: `src/data/repo.rs:116-142` (`repos()`), add setter after `set_repo_detail_bar_config` (~line 323)
- Modify every `Repo { … }` literal: `src/data/repo.rs` (test, ~391), `src/ui/pm_pane.rs:619`, `src/ui/dashboard/tests.rs:29`, `src/ui/detail_modules/mod.rs:128`, `src/ui/modal/updates_panel.rs:2167`, `src/agent/related.rs:103`, `src/git/github_remotes.rs:74`

**Interfaces:**
- Consumes: `HideRules::parse`, `HideRules::serialize` (Task 1)
- Produces: `Repo.hide_rules: crate::data::hide_rules::HideRules`; `Store::set_repo_hide_rules(&self, id: RepoId, rules: &HideRules) -> Result<()>`

- [ ] **Step 1: Write the failing tests** (in `src/data/store.rs` test module, beside `detail_bar_config_column_round_trips`)

```rust
    #[test]
    fn hide_rules_column_round_trips() {
        use crate::data::hide_rules::{HideRule, HideRules, PrCriterion};
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/some/repo"), "demo", "").unwrap();
        let load = |s: &Store| s.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert!(load(&store).hide_rules.is_empty());

        let rules = HideRules::parse(Some("pr:merged,pr:none"));
        store.set_repo_hide_rules(id, &rules).unwrap();
        assert!(load(&store).hide_rules.contains(HideRule::Pr(PrCriterion::Merged)));
        assert_eq!(load(&store).hide_rules, rules);

        store.set_repo_hide_rules(id, &HideRules::default()).unwrap();
        assert!(load(&store).hide_rules.is_empty());
    }

    #[test]
    fn migrate_v29_repo_hide_rules_column_idempotent() {
        let store = Store::open_in_memory().unwrap();
        store.migrate_for_test().unwrap();
        let n: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'hide_rules'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "hide_rules column must exist exactly once");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib hide_rules_column_round_trips migrate_v29`
Expected: compile error — no field `hide_rules`, no method `set_repo_hide_rules`.

- [ ] **Step 3: Implement**

`src/data/schema.rs`, after the `v < 28` block:

```rust
        if v < 29 {
            self.add_column_if_missing("repos", "hide_rules", "hide_rules TEXT")?;
            self.conn().execute("PRAGMA user_version = 29", [])?;
        }
```

`src/data/store.rs`, in `Repo` after `detail_bar_config`:

```rust
    /// Dashboard hide rules, parsed from the `hide_rules` column.
    pub hide_rules: crate::data::hide_rules::HideRules,
```

`src/data/repo.rs` `repos()`: extend the SELECT to `… created_at, sort_order, hide_rules` and add to the struct literal:

```rust
                hide_rules: crate::data::hide_rules::HideRules::parse(
                    r.get::<_, Option<String>>(13)?.as_deref(),
                ),
```

Setter, after `set_repo_detail_bar_config`:

```rust
    pub fn set_repo_hide_rules(
        &self,
        id: RepoId,
        rules: &crate::data::hide_rules::HideRules,
    ) -> Result<()> {
        self.conn().execute(
            "UPDATE repos SET hide_rules = ?1 WHERE id = ?2",
            rusqlite::params![rules.serialize(), id.0],
        )?;
        Ok(())
    }
```

Add `hide_rules: Default::default(),` after `detail_bar_config: None,` in each `Repo` literal listed above. Run `cargo build --all-targets` and add it anywhere else the compiler reports a missing field.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib hide_rules_column_round_trips migrate_v29 && cargo test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A src/
git commit -m "feat(data): persist per-repo hide rules"
```

---

### Task 3: Hide rows and draw summary rows in the by-repo view

**Files:**
- Modify: `src/app/types.rs:17-21`
- Modify: `src/app/selection.rs` (`selection_target_exists`, new `selected_repo`)
- Create: `src/ui/dashboard/hidden.rs`
- Modify: `src/ui/dashboard/mod.rs` (state, helpers, `render_by_repo`, `visible_targets` repo arm)
- Modify: `src/ui/dashboard/by_repo.rs` (`RepoView`, `header_line` fold glyph, `render_list`)
- Modify: `src/app/input/dashboard.rs` (exhaustive `match app.selected_target()` sites)
- Test: `src/ui/dashboard/tests.rs`

**Interfaces:**
- Consumes: `HideRule`, `HideRules::classify`, `HideSubject` (Task 1); `Repo.hide_rules` (Task 2)
- Produces:
  - `SelectionTarget::HiddenGroup { repo: RepoId, rule: HideRule }`
  - `DashboardState.expanded_hidden: HashSet<(RepoId, HideRule)>`
  - `pub(crate) fn hidden_by(w: &WorkspaceItem<'_>, filter: Option<&str>) -> Option<HideRule>` in `ui::dashboard`
  - `fn hidden_groups<'w, 'a>(repo: &Repo, workspaces: &'w [WorkspaceItem<'a>], filter: Option<&str>) -> Vec<(HideRule, Vec<&'w WorkspaceItem<'a>>)>`
  - `fn default_expanded(counts: StatusCounts, has_hidden: bool) -> bool`
  - `ui::dashboard::hidden::{HiddenGroupView, hidden_group_line, dimmed}`
  - `App::selected_repo(&self) -> Option<RepoId>`

- [ ] **Step 1: Write the failing tests** (append to `src/ui/dashboard/tests.rs`)

```rust
mod hide_rules {
    use super::*;
    use crate::data::hide_rules::{HideRule, HideRules, PrCriterion};
    use crate::git::forge::BranchLifecycle::{self, *};

    const MERGED: HideRule = HideRule::Pr(PrCriterion::Merged);
    const NO_PR: HideRule = HideRule::Pr(PrCriterion::NoPr);

    fn item<'a>(repo: &'a Repo, id: i64, branch: &str, lc: Option<BranchLifecycle>) -> WorkspaceItem<'a> {
        WorkspaceItem {
            repo,
            workspace_id: WorkspaceId(id),
            status: Status::Idle,
            row: row::RowInputs {
                agent: AgentKind::Claude,
                peers: Vec::new(),
                status: Status::Idle,
                branch: branch.to_string(),
                pr_number: None,
                procs: 0,
                diff: None,
                column: None,
                ago_secs: Some(id as u64),
                selected: false,
                yolo: false,
                badge: None,
                undelivered_mail: false,
                shared: false,
                shared_active: false,
                lifecycle: lc,
                review: None,
                unresolved: None,
                nerd_fonts: false,
                name_color: None,
                workspace_id: WorkspaceId(id),
                has_multi_pane_layout: false,
            },
        }
    }

    fn repo_with(rules: &str) -> Repo {
        let mut r = fake_repo(1, "wsx", "/r/wsx");
        r.hide_rules = HideRules::parse(Some(rules));
        r
    }

    /// open-pr and unknown-pr visible; two merged and one no-PR hidden.
    fn items(repo: &Repo) -> Vec<WorkspaceItem<'_>> {
        vec![
            item(repo, 1, "open-pr", Some(PrOpen)),
            item(repo, 2, "merged-a", Some(PrMerged)),
            item(repo, 3, "merged-b", Some(PrMerged)),
            item(repo, 4, "no-pr", Some(NoPr)),
            item(repo, 5, "unknown-pr", None),
        ]
    }

    fn inputs<'a>(repo: &'a Repo, ws: Vec<WorkspaceItem<'a>>) -> DashboardInputs<'a> {
        DashboardInputs {
            repos: vec![repo],
            workspaces: ws,
            activity: &[],
            column_widths: row::ColumnWidths::default(),
            github_remotes: Box::leak(Box::default()),
            nerd_fonts: false,
            fleet: crate::ui::bar::fleet::empty(),
        }
    }

    fn draw(inputs: &DashboardInputs<'_>, state: &mut DashboardState) -> String {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let mut term = Terminal::new(TestBackend::new(160, 30)).unwrap();
        term.draw(|f| render(f, f.area(), inputs, state, 0, &theme, &specs)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn line_with<'s>(screen: &'s str, needle: &str) -> &'s str {
        screen.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("no line with {needle:?}"))
    }

    #[test]
    fn by_repo_hides_matching_rows_and_draws_one_summary_row_per_rule() {
        let repo = repo_with("pr:merged,pr:none");
        let inputs = inputs(&repo, items(&repo));
        let screen = draw(&inputs, &mut DashboardState::default());
        assert!(screen.contains("open-pr"));
        assert!(!screen.contains("merged-a") && !screen.contains("merged-b") && !screen.contains("no-pr "));
        assert!(line_with(&screen, "merged").contains("2 hidden"));
        assert!(line_with(&screen, "no PR").contains("1 hidden"));
    }

    #[test]
    fn unknown_lifecycle_row_stays_visible() {
        let repo = repo_with("pr:merged,pr:none,pr:open");
        let inputs = inputs(&repo, items(&repo));
        assert!(draw(&inputs, &mut DashboardState::default()).contains("unknown-pr"));
    }

    #[test]
    fn header_counts_cover_visible_rows_only() {
        let repo = repo_with("pr:merged,pr:none");
        let inputs = inputs(&repo, items(&repo));
        let screen = draw(&inputs, &mut DashboardState::default());
        assert!(line_with(&screen, "/r/wsx").contains("2 ws"));
    }

    #[test]
    fn visible_targets_list_group_rows_after_visible_rows() {
        let repo = repo_with("pr:merged,pr:none");
        let inputs = inputs(&repo, items(&repo));
        let t = visible_targets(&inputs, &DashboardState::default());
        let rid = RepoId(1);
        assert_eq!(&t[0], &SelectionTarget::Repo(rid));
        assert_eq!(t.len(), 5, "{t:?}"); // header, 2 visible, 2 groups
        assert_eq!(t[3], SelectionTarget::HiddenGroup { repo: rid, rule: MERGED });
        assert_eq!(t[4], SelectionTarget::HiddenGroup { repo: rid, rule: NO_PR });
    }

    #[test]
    fn expanded_group_shows_its_rows_and_makes_them_selectable() {
        let repo = repo_with("pr:merged,pr:none");
        let inputs = inputs(&repo, items(&repo));
        let mut state = DashboardState::default();
        state.expanded_hidden.insert((RepoId(1), MERGED));
        let t = visible_targets(&inputs, &state);
        let g = t.iter().position(|x| *x == SelectionTarget::HiddenGroup { repo: RepoId(1), rule: MERGED }).unwrap();
        let mut after = t[g + 1..g + 3].to_vec();
        after.sort_by_key(|x| format!("{x:?}"));
        assert_eq!(after, vec![SelectionTarget::Workspace(WorkspaceId(2)), SelectionTarget::Workspace(WorkspaceId(3))]);
        let screen = draw(&inputs, &mut state);
        assert!(screen.contains("merged-a") && screen.contains("merged-b"));
        assert!(!screen.contains("no-pr "), "other groups stay collapsed");
    }

    #[test]
    fn active_filter_bypasses_hide_rules() {
        let repo = repo_with("pr:merged,pr:none");
        let inputs = inputs(&repo, items(&repo));
        let mut state = DashboardState { filter: Some("merged".into()), ..Default::default() };
        let screen = draw(&inputs, &mut state);
        assert!(screen.contains("merged-a"));
        assert!(!screen.contains("hidden"));
        assert!(!visible_targets(&inputs, &state).iter().any(|t| matches!(t, SelectionTarget::HiddenGroup { .. })));
    }

    #[test]
    fn all_hidden_repo_keeps_header_and_summary_rows() {
        let repo = repo_with("pr:merged");
        let ws = vec![item(&repo, 2, "merged-a", Some(PrMerged))];
        let inputs = inputs(&repo, ws);
        let t = visible_targets(&inputs, &DashboardState::default());
        assert_eq!(t, vec![
            SelectionTarget::Repo(RepoId(1)),
            SelectionTarget::HiddenGroup { repo: RepoId(1), rule: MERGED },
        ]);
        let screen = draw(&inputs, &mut DashboardState::default());
        assert!(line_with(&screen, "/r/wsx").contains('▾'), "header expanded");
        assert!(line_with(&screen, "merged").contains("1 hidden"));
    }
}
```

If `GithubRemotes` lacks `Default`, use the same expression `render_to_strings` uses (`&Default::default()` bound to a local in the test).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::dashboard::tests::hide_rules`
Expected: compile errors — `HiddenGroup`, `expanded_hidden` missing.

- [ ] **Step 3: Add the selection target and state**

`src/app/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionTarget {
    Repo(crate::data::store::RepoId),
    Workspace(crate::data::store::WorkspaceId),
    /// A repo's summary row for workspaces hidden by `rule`.
    HiddenGroup {
        repo: crate::data::store::RepoId,
        rule: crate::data::hide_rules::HideRule,
    },
}
```

`src/app/selection.rs` — in `selection_target_exists` add
`SelectionTarget::HiddenGroup { repo, .. } => self.repos.iter().any(|r| r.id == repo),`
and add to `impl App`:

```rust
    /// The repo the selection belongs to: the header itself, a workspace's
    /// owner, or a hidden group's repo.
    pub(crate) fn selected_repo(&self) -> Option<crate::data::store::RepoId> {
        match self.selected_target()? {
            SelectionTarget::Repo(id) | SelectionTarget::HiddenGroup { repo: id, .. } => Some(id),
            SelectionTarget::Workspace(wid) => self
                .workspaces
                .iter()
                .find(|(_, w)| w.id == wid)
                .map(|(rid, _)| *rid),
        }
    }
```

`src/ui/dashboard/mod.rs` — `DashboardState` gains, after `folded`:

```rust
    /// Hidden groups the user expanded this session. Not persisted.
    pub expanded_hidden: std::collections::HashSet<(crate::data::store::RepoId, crate::data::hide_rules::HideRule)>,
```

and `expanded_hidden: Default::default(),` in `Default`.

- [ ] **Step 4: Fix exhaustive matches in input**

Run `cargo build --all-targets`. In `src/app/input/dashboard.rs`, replace the repo-resolving matches in `toggle_focused_fold`, `set_focused_fold`, the `n`/`N` arm, and the `s` arm with `app.selected_repo()` (keeping their existing `None => app.repos.first()` fallback where present, e.g. `app.selected_repo().or_else(|| app.repos.first().map(|r| r.id))`). For the `l` and `Enter`/`i` arms add `Some(SelectionTarget::HiddenGroup { .. }) => {}` — Task 5 gives them behavior. Fix any other exhaustive match the compiler reports the same way (resolve to repo where the arm is repo-scoped, otherwise no-op).

- [ ] **Step 5: Shared hidden-row rendering**

Create `src/ui/dashboard/hidden.rs` and add `pub mod hidden;` to `src/ui/dashboard/mod.rs`:

```rust
//! Summary rows for workspaces a repo's hide rules keep off the dashboard.

use crate::data::hide_rules::HideRule;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// One rule's hidden rows within a repo, already ordered for display.
#[derive(Debug, Clone)]
pub struct HiddenGroupView {
    pub rule: HideRule,
    pub expanded: bool,
    pub workspaces: Vec<RowInputs>,
}

/// `  ▸ ⎇ merged  ·  4 hidden`, optionally prefixed by a repo name (by-attention view).
pub fn hidden_group_line(
    g: &HiddenGroupView,
    repo_prefix: Option<&str>,
    nerd_fonts: bool,
    theme: &Theme,
) -> Line<'static> {
    let lc = g.rule.lifecycle();
    let fold = if g.expanded { '▾' } else { '▸' };
    let mut spans = vec![Span::styled(format!("  {fold} "), theme.dim_style())];
    if let Some(name) = repo_prefix {
        spans.push(Span::styled(format!("{name} · "), theme.dim_style()));
    }
    spans.push(Span::styled(
        format!("{} ", crate::ui::theme::branch_glyph(lc, nerd_fonts)),
        theme.lifecycle_style(lc).unwrap_or_else(|| theme.dim_style()),
    ));
    spans.push(Span::styled(
        format!("{}  ·  {} hidden", g.rule.label(), g.workspaces.len()),
        theme.dim_style(),
    ));
    Line::from(spans)
}

/// Fade an expanded hidden row so it reads as revealed, not restored.
pub fn dimmed(line: Line<'static>) -> Line<'static> {
    line.patch_style(Style::default().add_modifier(Modifier::DIM))
}
```

- [ ] **Step 6: Visibility helpers in `src/ui/dashboard/mod.rs`** (next to `matches_filter`)

```rust
/// The rule hiding `w`, or `None` when it shows normally. An active filter
/// bypasses every rule so a search can find any row.
pub(crate) fn hidden_by(w: &WorkspaceItem<'_>, filter: Option<&str>) -> Option<HideRule> {
    if filter.is_some() {
        return None;
    }
    w.repo.hide_rules.classify(&HideSubject { lifecycle: w.row.lifecycle })
}

/// `repo`'s hidden rows grouped by rule, in the repo's rule order. Empty
/// groups are omitted.
fn hidden_groups<'w, 'a>(
    repo: &Repo,
    workspaces: &'w [WorkspaceItem<'a>],
    filter: Option<&str>,
) -> Vec<(HideRule, Vec<&'w WorkspaceItem<'a>>)> {
    repo.hide_rules
        .iter()
        .filter_map(|rule| {
            let rows: Vec<_> = workspaces
                .iter()
                .filter(|w| w.repo.id == repo.id && hidden_by(w, filter) == Some(rule))
                .collect();
            (!rows.is_empty()).then_some((rule, rows))
        })
        .collect()
}

/// Default fold: a repo with no rows at all starts folded; one whose rows
/// are all hidden stays open so its summary rows show.
fn default_expanded(counts: StatusCounts, has_hidden: bool) -> bool {
    !default_fold(counts) || has_hidden
}
```

Add imports: `use crate::data::hide_rules::{HideRule, HideSubject};` and `use crate::ui::dashboard::hidden::HiddenGroupView;`.

- [ ] **Step 7: Wire `render_by_repo`**

In the per-repo closure, filter visible rows with `.filter(|w| hidden_by(w, filter).is_none())` after the existing `matches_filter` filter, then build groups:

```rust
            let hidden: Vec<HiddenGroupView> = hidden_groups(r, &inputs.workspaces, filter)
                .into_iter()
                .map(|(rule, rows)| {
                    let mut workspaces: Vec<RowInputs> = rows.iter().map(|w| w.row.clone()).collect();
                    order_workspaces(&mut workspaces, state.sort_mode, state.blocked_pin_max_age_secs);
                    HiddenGroupView {
                        rule,
                        expanded: state.expanded_hidden.contains(&(r.id, rule)),
                        workspaces,
                    }
                })
                .collect();
```

Replace `None => !default_fold(counts)` with `None => default_expanded(counts, !hidden.is_empty())`, and pass `hidden` into `RepoView`. Extend the selection walk after the visible-workspace loop (before the spacer):

```rust
        for g in &mut view.hidden {
            if selection == Some(SelectionTarget::HiddenGroup { repo: RepoId(view.id as i64), rule: g.rule }) {
                selected_idx = Some(flat_idx);
            }
            flat_idx += 1;
            if !g.expanded {
                continue;
            }
            for w in &mut g.workspaces {
                if selection == Some(SelectionTarget::Workspace(w.workspace_id)) {
                    selected_idx = Some(flat_idx);
                    w.selected = true;
                }
                if let Some(span) = row::pr_chip_hit_span(w, widths) {
                    chip_spans.push((w.workspace_id, flat_idx, span));
                }
                flat_idx += 1;
            }
        }
```

Include expanded hidden rows in `derived_agent_width`: chain `v.hidden.iter().filter(|g| g.expanded).flat_map(|g| g.workspaces.iter())` into the expanded-view iterator.

- [ ] **Step 8: `RepoView` and `render_list` in `by_repo.rs`**

Add `pub hidden: Vec<crate::ui::dashboard::hidden::HiddenGroupView>,` to `RepoView` (update the test `make_view` helper with `hidden: Vec::new()`). In `header_line`, change the blank-glyph condition to `view.counts.total() == 0 && view.hidden.is_empty()`. In `render_list`, after the visible-row loop and before the spacer:

```rust
        for g in &view.hidden {
            items.push(ListItem::new(crate::ui::dashboard::hidden::hidden_group_line(
                g, None, view.nerd_fonts, theme,
            )));
            if g.expanded {
                for w in &g.workspaces {
                    items.push(ListItem::new(crate::ui::dashboard::hidden::dimmed(
                        row::render(w, widths, tick, theme, width),
                    )));
                }
            }
        }
```

- [ ] **Step 9: `visible_targets` repo arm**

Filter `shown` with `&& hidden_by(w, filter).is_none()`. In the repo loop, compute `let groups = hidden_groups(repo, &inputs.workspaces, filter);` (look up `repo` in `inputs.repos` by `repo_id`), use `default_expanded(s.counts, !groups.is_empty())`, and inside `if expanded` after the visible ids:

```rust
                    for (rule, rows) in &groups {
                        out.push(SelectionTarget::HiddenGroup { repo: repo_id, rule: *rule });
                        if state.expanded_hidden.contains(&(repo_id, *rule)) {
                            let mut nav: Vec<NavRow> = rows.iter().map(|w| NavRow::from(*w)).collect();
                            order_workspaces(&mut nav, state.sort_mode, state.blocked_pin_max_age_secs);
                            out.extend(nav.into_iter().map(|n| SelectionTarget::Workspace(n.workspace_id)));
                        }
                    }
```

For the attention arm in this task, only apply the same `shown` filter; Task 4 adds its groups.

- [ ] **Step 10: Run tests**

Run: `cargo test --lib ui::dashboard && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 11: Commit**

```bash
git add -A src/
git commit -m "feat(dashboard): hide rule-matched workspaces behind summary rows"
```

---

### Task 4: Hidden section in the by-attention view

**Files:**
- Modify: `src/ui/dashboard/by_attention.rs` (`AttentionData`, `partition`, `render_list`)
- Modify: `src/ui/dashboard/mod.rs` (`render_by_attention`, `quiet_repo_names`, `visible_targets` attention arm)
- Test: `src/ui/dashboard/tests.rs` (`hide_rules` module)

**Interfaces:**
- Consumes: `hidden_by`, `hidden_groups`, `HiddenGroupView`, `hidden_group_line`, `dimmed`, `SelectionTarget::HiddenGroup` (Task 3)
- Produces: `AttentionData.hidden: Vec<(String /* repo name */, RepoId, HiddenGroupView)>`, rendered as a `  HIDDEN` section between IDLE and QUIET REPOS, ordered by repo `(sort_order, id)` then rule.

- [ ] **Step 1: Write the failing tests** (in the `hide_rules` test module)

```rust
    #[test]
    fn attention_view_lists_hidden_groups_in_their_own_section() {
        let repo = repo_with("pr:merged,pr:none");
        let inputs = inputs(&repo, items(&repo));
        let mut state = DashboardState { group_mode: GroupMode::Attention, ..Default::default() };
        let screen = draw(&inputs, &mut state);
        assert!(screen.contains("HIDDEN"));
        assert!(line_with(&screen, "wsx · ").contains("2 hidden"));
        assert!(!screen.contains("merged-a"));
        let t = visible_targets(&inputs, &state);
        assert_eq!(
            t.iter().filter(|x| matches!(x, SelectionTarget::HiddenGroup { .. })).count(),
            2
        );
    }

    #[test]
    fn all_hidden_repo_is_not_quiet() {
        let repo = repo_with("pr:merged");
        let inputs = inputs(&repo, vec![item(&repo, 2, "merged-a", Some(PrMerged))]);
        let mut state = DashboardState { group_mode: GroupMode::Attention, ..Default::default() };
        let screen = draw(&inputs, &mut state);
        assert!(!screen.contains("QUIET REPOS"), "{screen}");
        assert!(screen.contains("HIDDEN"));
    }

    #[test]
    fn attention_expanded_group_rows_are_selectable() {
        let repo = repo_with("pr:merged");
        let inputs = inputs(&repo, items(&repo));
        let mut state = DashboardState { group_mode: GroupMode::Attention, ..Default::default() };
        state.expanded_hidden.insert((RepoId(1), MERGED));
        let t = visible_targets(&inputs, &state);
        assert!(t.contains(&SelectionTarget::Workspace(WorkspaceId(2))));
        assert!(draw(&inputs, &mut state).contains("merged-a"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::dashboard::tests::hide_rules`
Expected: the three new tests fail (no HIDDEN section; repo listed as quiet).

- [ ] **Step 3: Implement**

`by_attention.rs`: add `pub hidden: Vec<(String, crate::data::store::RepoId, crate::ui::dashboard::hidden::HiddenGroupView)>,` to `AttentionData`; `partition` sets `hidden: Vec::new()`. In `render_list`, between IDLE and QUIET REPOS:

```rust
    if !data.hidden.is_empty() {
        let total: usize = data.hidden.iter().map(|(_, _, g)| g.workspaces.len()).sum();
        items.push(ListItem::new(section_header(
            "  HIDDEN",
            total,
            None,
            Style::default().fg(theme.path),
            width,
            theme,
        )));
        for (repo_name, _, g) in &data.hidden {
            items.push(ListItem::new(crate::ui::dashboard::hidden::hidden_group_line(
                g,
                Some(repo_name),
                g.workspaces.first().is_some_and(|w| w.nerd_fonts),
                theme,
            )));
            if g.expanded {
                for w in &g.workspaces {
                    let flat = FlatRow { repo_name: repo_name.clone(), row: w.clone() };
                    items.push(ListItem::new(crate::ui::dashboard::hidden::dimmed(
                        flat_row_line(&flat, widths, tick, theme, width),
                    )));
                }
            }
        }
    }
```

`mod.rs`:
- Add a helper used by both the renderer and `visible_targets` so they share one order:

```rust
/// Every repo's hidden groups in display order: repos by `(sort_order, id)`, then rule.
fn all_hidden_groups<'w, 'a>(
    inputs: &'w DashboardInputs<'a>,
    filter: Option<&str>,
) -> Vec<(&'w Repo, HideRule, Vec<&'w WorkspaceItem<'a>>)> {
    let mut repos: Vec<&Repo> = inputs.repos.to_vec();
    repos.sort_by_key(|r| (r.sort_order, r.id.0));
    repos
        .into_iter()
        .flat_map(|r| {
            hidden_groups(r, &inputs.workspaces, filter)
                .into_iter()
                .map(move |(rule, rows)| (r, rule, rows))
        })
        .collect()
}
```

- `render_by_attention`: add `.filter(|w| hidden_by(w, filter).is_none())` to `rows` and to the per-repo `repo_rows`; compute `let has_hidden = !hidden_groups(r, &inputs.workspaces, filter).is_empty();` and change `include_empty` to `count == 0 && !has_hidden && (filter.is_none() || repo_matches_filter)`. After `partition`, fill `data.hidden` from `all_hidden_groups` (rows cloned and ordered with `order_workspaces` under `state.sort_mode`, `expanded` from `state.expanded_hidden`). Include expanded hidden rows in `derived_agent_width`. Extend the selection walk after the four sections:

```rust
    if !data.hidden.is_empty() {
        flat_idx += 1; // HIDDEN header
        for (_, repo_id, g) in &mut data.hidden {
            if selection == Some(SelectionTarget::HiddenGroup { repo: *repo_id, rule: g.rule }) {
                selected_idx = Some(flat_idx);
            }
            flat_idx += 1;
            if !g.expanded {
                continue;
            }
            for w in &mut g.workspaces {
                if selection == Some(SelectionTarget::Workspace(w.workspace_id)) {
                    selected_idx = Some(flat_idx);
                    w.selected = true;
                }
                if let Some(span) = row::pr_chip_hit_span(w, widths) {
                    chip_spans.push((w.workspace_id, flat_idx, span));
                }
                flat_idx += 1;
            }
        }
    }
```

- `quiet_repo_names`: same `hidden_by` filter on `repo_rows` and the same `!has_hidden` condition.
- `visible_targets` attention arm: after the section loop, append the same targets Task 3 emits per group, iterating `all_hidden_groups(inputs, filter)`.

- [ ] **Step 4: Run tests**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A src/
git commit -m "feat(dashboard): show hidden groups in the attention view"
```

---

### Task 5: Expand, collapse, and selection fallback

**Files:**
- Modify: `src/app/selection.rs` (`hide_subject`, `hidden_group_of`, `select_workspace_by_name`)
- Modify: `src/app/input/dashboard.rs` (`h`, `l`, `Enter`/`i` arms)
- Modify: `src/app/render/dashboard.rs:86-102` (before `reconcile_selection`)
- Create: `src/app/input/tests/hide_rules.rs`; add `mod hide_rules;` to `src/app/input/tests/mod.rs`

**Interfaces:**
- Consumes: `SelectionTarget::HiddenGroup`, `DashboardState.expanded_hidden`, `App::selected_repo` (Task 3); `HideSubject`, `HideRules::classify` (Task 1)
- Produces: `App::hide_subject(&self, ws: WorkspaceId) -> HideSubject`; `App::hidden_group_of(&self, ws: WorkspaceId) -> Option<(RepoId, HideRule)>` (ignores the `/` filter)

- [ ] **Step 1: Write the failing tests** (`src/app/input/tests/hide_rules.rs`)

```rust
//! Hidden-group expand/collapse, selection fallback, and the `F` picker.

use super::common::*;
use super::*;
use crate::data::hide_rules::{HideRule, HideRules, PrCriterion};
use crate::git::forge::BranchLifecycle;
use crossterm::event::KeyModifiers;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const MERGED: HideRule = HideRule::Pr(PrCriterion::Merged);

/// One repo hiding merged PRs; `first` is merged, `second` open.
fn app_with_merged_hidden() -> (App, crate::data::store::RepoId, Vec<crate::data::store::WorkspaceId>) {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let repo = app.repos[0].id;
    app.store
        .set_repo_hide_rules(repo, &HideRules::parse(Some("pr:merged")))
        .unwrap();
    app.refresh().unwrap();
    let ids: Vec<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    app.pr_lifecycle.insert(ids[0], BranchLifecycle::PrMerged);
    app.pr_lifecycle.insert(ids[1], BranchLifecycle::PrOpen);
    (app, repo, ids)
}

fn draw(app: &mut App) {
    let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, app)).unwrap();
}

fn select(app: &mut App, t: SelectionTarget) {
    let idx = app.selectable.iter().position(|x| *x == t).expect("target visible");
    app.select_index(idx);
}

#[tokio::test]
async fn enter_expands_and_h_collapses_a_hidden_group() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    draw(&mut app);
    let group = SelectionTarget::HiddenGroup { repo, rule: MERGED };
    select(&mut app, group);
    press_key(&mut app, KeyCode::Enter).await;
    assert!(app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[0])));

    select(&mut app, SelectionTarget::Workspace(ids[0]));
    press(&mut app, 'h', KeyModifiers::NONE).await;
    assert!(!app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
    assert_eq!(app.selected_target(), Some(group), "h lands on the group row");
}

#[tokio::test]
async fn l_expands_a_hidden_group() {
    let (mut app, repo, _) = app_with_merged_hidden();
    draw(&mut app);
    select(&mut app, SelectionTarget::HiddenGroup { repo, rule: MERGED });
    press(&mut app, 'l', KeyModifiers::NONE).await;
    assert!(app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
}

#[tokio::test]
async fn selected_workspace_falls_back_to_its_group_row_when_hidden() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    app.pr_lifecycle.insert(ids[0], BranchLifecycle::PrOpen);
    draw(&mut app);
    select(&mut app, SelectionTarget::Workspace(ids[0]));
    app.pr_lifecycle.insert(ids[0], BranchLifecycle::PrMerged); // PR merges
    draw(&mut app);
    assert_eq!(
        app.selected_target(),
        Some(SelectionTarget::HiddenGroup { repo, rule: MERGED })
    );
}

#[tokio::test]
async fn select_by_name_reveals_hidden_group() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    draw(&mut app);
    assert!(app.select_workspace_by_name("repo", "first"));
    assert!(app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
    draw(&mut app);
    assert_eq!(app.selected_target(), Some(SelectionTarget::Workspace(ids[0])));
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[0])));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::input::tests::hide_rules`
Expected: FAIL — Enter/l are no-ops, `h` folds the repo, fallback parks, select-by-name returns false.

- [ ] **Step 3: App helpers** (`src/app/selection.rs`, in `impl App`)

```rust
    /// The facts hide rules test for `ws`, from the app's caches.
    pub(crate) fn hide_subject(
        &self,
        ws: crate::data::store::WorkspaceId,
    ) -> crate::data::hide_rules::HideSubject {
        crate::data::hide_rules::HideSubject {
            lifecycle: self.pr_lifecycle.get(&ws).copied(),
        }
    }

    /// The group whose rule hides `ws`, ignoring any active `/` filter.
    pub(crate) fn hidden_group_of(
        &self,
        ws: crate::data::store::WorkspaceId,
    ) -> Option<(crate::data::store::RepoId, crate::data::hide_rules::HideRule)> {
        let rid = self.workspaces.iter().find(|(_, w)| w.id == ws).map(|(r, _)| *r)?;
        let repo = self.repos.iter().find(|r| r.id == rid)?;
        repo.hide_rules
            .classify(&self.hide_subject(ws))
            .map(|rule| (rid, rule))
    }
```

In `select_workspace_by_name`, after `self.dashboard.folded.insert(...)`:

```rust
        // A hidden workspace isn't in `selectable` until its group expands;
        // expand it and let the next draw's reconcile resolve the index.
        if let Some(group) = self.hidden_group_of(ws_id) {
            self.dashboard.expanded_hidden.insert(group);
            if !self.selectable.contains(&SelectionTarget::Workspace(ws_id)) {
                self.dashboard.selection = Some(SelectionTarget::Workspace(ws_id));
                return true;
            }
        }
```

- [ ] **Step 4: Input arms** (`src/app/input/dashboard.rs`)

Replace the `h` arm and the `HiddenGroup` placeholders from Task 3:

```rust
        (KeyCode::Char('h'), _) => match app.selected_target() {
            Some(SelectionTarget::HiddenGroup { repo, rule }) => {
                app.dashboard.expanded_hidden.remove(&(repo, rule));
            }
            Some(SelectionTarget::Workspace(id))
                if app
                    .hidden_group_of(id)
                    .is_some_and(|g| app.dashboard.expanded_hidden.contains(&g)) =>
            {
                let (repo, rule) = app.hidden_group_of(id).unwrap();
                app.dashboard.expanded_hidden.remove(&(repo, rule));
                app.dashboard.selection = Some(SelectionTarget::HiddenGroup { repo, rule });
            }
            _ => set_focused_fold(app, true),
        },
```

In the `l` arm and the `Enter`/`i` arm:

```rust
            Some(SelectionTarget::HiddenGroup { repo, rule }) => {
                app.dashboard.expanded_hidden.insert((repo, rule));
            }
```

- [ ] **Step 5: Fallback before reconcile** (`src/app/render/dashboard.rs`, right after `let new_selectable = …`)

```rust
    // A selected row that a hide rule just swallowed lands on its group's
    // summary row rather than parking invisibly.
    if let Some(SelectionTarget::Workspace(id)) = app.dashboard.selection {
        if !new_selectable.contains(&SelectionTarget::Workspace(id)) {
            if let Some((repo, rule)) = app.hidden_group_of(id) {
                let group = SelectionTarget::HiddenGroup { repo, rule };
                if new_selectable.contains(&group) {
                    app.dashboard.selection = Some(group);
                }
            }
        }
    }
```

Make sure this runs before `prev_selection` is read.

- [ ] **Step 6: Run tests**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add -A src/
git commit -m "feat(dashboard): expand and collapse hidden workspace groups"
```

---

### Task 6: `F` hide-rules picker

**Files:**
- Modify: `src/ui/modal/mod.rs` (variant, `render` arm)
- Modify: `src/app/input/modal/mod.rs` (dispatch arm)
- Modify: `src/app/input/modal/pickers.rs` (handler)
- Modify: `src/app/input/dashboard.rs` (`F` arm)
- Test: `src/app/input/tests/hide_rules.rs`

**Interfaces:**
- Consumes: `App::selected_repo`, `App::hide_subject` (Tasks 3, 5); `HideRule::ALL`, `HideRules::toggle` (Task 1); `Store::set_repo_hide_rules` (Task 2)
- Produces: `Modal::HideRules { repo_id: RepoId, repo_name: String, rules: HideRules, counts: [usize; 6], selected: usize }` — `counts[i]` is how many of the repo's workspaces `HideRule::ALL[i]` matches, snapshotted on open.

- [ ] **Step 1: Write the failing tests** (append to `src/app/input/tests/hide_rules.rs`)

```rust
async fn open_picker(app: &mut App) {
    draw(app);
    let repo = app.repos[0].id;
    select(app, SelectionTarget::Repo(repo));
    press(app, 'F', KeyModifiers::NONE).await;
}

#[tokio::test]
async fn f_opens_picker_for_selected_repo_with_counts() {
    let (mut app, repo, _) = app_with_merged_hidden();
    open_picker(&mut app).await;
    let Some(crate::ui::modal::Modal::HideRules { repo_id, counts, rules, .. }) = &app.modal else {
        panic!("expected HideRules modal, got {:?}", app.modal);
    };
    assert_eq!(*repo_id, repo);
    assert_eq!(counts[0], 1, "one merged workspace"); // ALL[0] is Merged
    assert!(rules.contains(MERGED));
}

#[tokio::test]
async fn space_toggles_and_persists() {
    let (mut app, repo, _) = app_with_merged_hidden();
    open_picker(&mut app).await;
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Char(' '))).await.unwrap();
    let stored = app.store.repos().unwrap().into_iter().find(|r| r.id == repo).unwrap();
    assert!(!stored.hide_rules.contains(MERGED), "space on row 0 toggles merged off");
    assert!(matches!(app.modal, Some(crate::ui::modal::Modal::HideRules { .. })), "picker stays open");
}

#[tokio::test]
async fn toggling_rule_off_forgets_expansion() {
    let (mut app, repo, _) = app_with_merged_hidden();
    app.dashboard.expanded_hidden.insert((repo, MERGED));
    open_picker(&mut app).await;
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Char(' '))).await.unwrap();
    assert!(!app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
}

#[tokio::test]
async fn esc_closes_picker() {
    let (mut app, _, _) = app_with_merged_hidden();
    open_picker(&mut app).await;
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Esc)).await.unwrap();
    assert!(app.modal.is_none());
}
```

If `handle_key_modal` isn't reachable at that path from the tests module, call it the way `src/app/input/tests/rename_modal.rs` does.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::input::tests::hide_rules`
Expected: compile error — no `Modal::HideRules`.

- [ ] **Step 3: Modal variant and render** (`src/ui/modal/mod.rs`)

Add to `Modal`:

```rust
    /// The `F` checklist of hide rules for one repo. `counts[i]` is how many
    /// of its workspaces `HideRule::ALL[i]` matches, snapshotted on open.
    HideRules {
        repo_id: crate::data::store::RepoId,
        repo_name: String,
        rules: crate::data::hide_rules::HideRules,
        counts: [usize; 6],
        selected: usize,
    },
```

In `render`'s `(title, body)` match:

```rust
        Modal::HideRules { repo_name, rules, counts, selected, .. } => {
            let list = crate::data::hide_rules::HideRule::ALL
                .iter()
                .enumerate()
                .map(|(i, rule)| {
                    let cursor = if i == *selected { ">" } else { " " };
                    let mark = if rules.contains(*rule) { "x" } else { " " };
                    format!("{cursor} [{mark}] {:<12} {:>3}", rule.label(), counts[i])
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                "hide workspaces",
                format!(
                    "Hide {repo_name} workspaces whose PR is:\n\n{list}\n\n\
                     \u{2191}\u{2193} move   space toggle   Esc close"
                ),
            )
        }
```

(`title` may need to be a `String` if the match already uses owned titles — follow the surrounding arms.)

- [ ] **Step 4: Handler** (`src/app/input/modal/pickers.rs`)

```rust
#[allow(clippy::too_many_arguments)]
pub(super) async fn hide_rules_picker(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    repo_id: crate::data::store::RepoId,
    repo_name: String,
    mut rules: crate::data::hide_rules::HideRules,
    counts: [usize; 6],
    selected: usize,
) -> Result<()> {
    use crate::data::hide_rules::HideRule;
    let last = HideRule::ALL.len() - 1;
    let selected = match k.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.modal = None;
            return Ok(());
        }
        KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => (selected + 1).min(last),
        KeyCode::Char(' ') => {
            let rule = HideRule::ALL[selected];
            rules.toggle(rule);
            if let Err(e) = app.store.set_repo_hide_rules(repo_id, &rules) {
                app.modal = Some(Modal::Error {
                    message: format!("could not save hide rules: {e}"),
                });
                return Ok(());
            }
            if !rules.contains(rule) {
                app.dashboard.expanded_hidden.remove(&(repo_id, rule));
            }
            app.refresh()?;
            selected
        }
        _ => selected,
    };
    app.modal = Some(Modal::HideRules { repo_id, repo_name, rules, counts, selected });
    Ok(())
}
```

Dispatch arm in `src/app/input/modal/mod.rs`:

```rust
        Modal::HideRules { repo_id, repo_name, rules, counts, selected } => {
            pickers::hide_rules_picker(app, k, repo_id, repo_name, rules, counts, selected).await?
        }
```

Fix any other exhaustive `Modal` match the compiler reports (treat it like `RemoteHostPicker`).

- [ ] **Step 5: `F` key** (`src/app/input/dashboard.rs`, near the `C` arm)

```rust
        // `F` opens the hide-rules checklist for the selected row's repo.
        (KeyCode::Char('F'), _) => {
            if let Some(repo_id) = app.selected_repo() {
                if let Some(repo) = app.repos.iter().find(|r| r.id == repo_id) {
                    let ids: Vec<_> = app
                        .workspaces
                        .iter()
                        .filter(|(rid, _)| *rid == repo_id)
                        .map(|(_, w)| w.id)
                        .collect();
                    let counts = crate::data::hide_rules::HideRule::ALL.map(|rule| {
                        ids.iter().filter(|id| rule.matches(&app.hide_subject(**id))).count()
                    });
                    app.modal = Some(Modal::HideRules {
                        repo_id,
                        repo_name: repo.name.clone(),
                        rules: repo.hide_rules.clone(),
                        counts,
                        selected: 0,
                    });
                }
            }
        }
```

- [ ] **Step 6: Run tests**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add -A src/
git commit -m "feat(dashboard): add F picker to edit a repo's hide rules"
```

---

### Task 7: Updates panel respects hide rules

**Files:**
- Modify: `src/ui/modal/updates_panel.rs:151-177` (`panel_sections`), tests in `ordering_tests`

**Interfaces:**
- Consumes: `crate::ui::dashboard::hidden_by` (Task 3)

- [ ] **Step 1: Write the failing tests** (in `ordering_tests`)

```rust
    #[test]
    fn hide_rules_drop_rows_unless_filtering() {
        use crate::git::forge::BranchLifecycle;
        let mut repo = fixture_repo(1);
        repo.hide_rules = crate::data::hide_rules::HideRules::parse(Some("pr:merged"));
        let repos = vec![repo];
        let ws = vec![fixture_ws(1, 1, "kept"), fixture_ws(2, 1, "gone")];
        let maps = Maps::default();
        let mut pi = inputs(&repos, &ws, &maps);
        pi.items.iter_mut().find(|i| i.workspace_id == WorkspaceId(2)).unwrap().row.lifecycle =
            Some(BranchLifecycle::PrMerged);
        assert_eq!(ordered_workspaces_for_panel(&pi, None), vec![WorkspaceId(1)]);
        assert!(ordered_workspaces_for_panel(&pi, Some("gone")).contains(&WorkspaceId(2)));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib hide_rules_drop_rows_unless_filtering`
Expected: FAIL — left `[1, 2]`.

- [ ] **Step 3: Implement** — in `panel_sections`, make the item filter:

```rust
        .filter(|item| {
            let Some(n) = needle else {
                return crate::ui::dashboard::hidden_by(item, None).is_none();
            };
            inputs
                .workspace(item.workspace_id)
                .is_some_and(|w| matches_filter(w, &item.repo.name, &inputs.status_text(w), n))
        })
```

Update the `panel_sections` doc comment with one line: hide rules apply, except while filtering.

- [ ] **Step 4: Run tests**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ui/modal/updates_panel.rs
git commit -m "feat(updates-panel): respect repo hide rules"
```

---

### Task 8: Docs

**Files:**
- Modify: `docs/book/src/daily-use/keybindings.md` (dashboard table, after the `s` row)
- Create: `docs/book/src/daily-use/hiding-workspaces.md`
- Modify: `docs/book/src/SUMMARY.md` (after "Workspace updates panel")
- Modify: `docs/superpowers/specs/2026-10-15-workspace-hide-rules-design.md` (module path and selection fallback, to match the plan)

- [ ] **Step 1: Keybindings rows**

```markdown
| `Shift + F`                       | Choose which PR states to hide for the selected row's repo (persisted per repo). See [Hiding workspaces](hiding-workspaces.md) |
| `enter` / `l` on a hidden-group row | Show that group's workspaces inline (this session only)                                                    |
| `h` on a hidden-group row or a row inside it | Collapse the group back to its summary row                                                      |
```

Pad cells to the table's existing column widths.

- [ ] **Step 2: New page** (`hiding-workspaces.md`)

```markdown
# Hiding workspaces

Long-lived workspaces can crowd a repo's section once their PRs merge or if
they never get one. Each repo can hide workspaces by PR state.

Press `F` on a repo header (or any of its rows) and toggle states with
`space`: no PR, draft, open, conflicted, merged, closed. The choice is saved
per repo.

Hidden workspaces collapse into one dimmed row per state under their repo:

    ▸ ⏺ merged  ·  4 hidden

Press `enter` or `l` on that row to show its workspaces inline; `h` collapses
it again. Expansion lasts until wsx restarts.

A few things stay predictable:

- Typing a `/` filter searches every workspace, hidden or not.
- A workspace whose PR state hasn't loaded yet is never hidden.
- If a selected workspace becomes hidden (say its PR merges), the selection
  moves to its group's row.
- Status bar, waybar, and menubar counts still include hidden workspaces.
- The updates panel (`Ctrl-x u`) leaves hidden workspaces out.
```

- [ ] **Step 3: SUMMARY entry** — `  - [Hiding workspaces](daily-use/hiding-workspaces.md)` after the updates-panel line.

- [ ] **Step 4: Spec touch-up** — change "New module `src/ui/dashboard/hide_rules.rs`" to `src/data/hide_rules.rs` (rule model; glyph/style come from the dashboard via `HideRule::lifecycle()`), and replace the "existing parking logic moves selection to a neighbor" sentence with: selection moves to the workspace's group summary row.

- [ ] **Step 5: Build the book if mdbook is installed**

Run: `command -v mdbook && mdbook build docs/book`
Expected: builds without broken-link warnings (skip if mdbook is absent).

- [ ] **Step 6: Commit**

```bash
git add docs/
git commit -m "docs: document per-repo workspace hide rules"
```
