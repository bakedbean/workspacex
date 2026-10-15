# Workspace Tags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Free-text tags on workspaces (TUI `#` editor + `wsx workspace tag` CLI), shown on dashboard rows, searchable with `/`, and usable as per-repo hide rules alongside PR-state rules.

**Architecture:** Tags live in a new `workspace_tags` table (schema v30) and load onto `Workspace.tags`. `HideRule` gains a `Tag(Arc<str>)` variant declared before `Pr`, so derived `Ord` gives tag rules precedence in first-match classification; `HideRule`, `HideSubject` and `SelectionTarget` lose `Copy`. The `F` picker's fixed `[usize; 6]` counts become dynamic rows.

**Tech Stack:** Rust, rusqlite, ratatui, crossterm, tokio tests.

**Spec:** `docs/superpowers/specs/2026-10-15-workspace-tags-design.md`

## Global Constraints

- Tag names: trim, lowercase, then `[a-z0-9_-]{1,32}`; all entry points go through `crate::data::tags::normalize_tag`.
- Rule token for tags: `tag:<name>`; stored in `repos.hide_rules` (format unchanged). Unknown/invalid tokens are skipped.
- Tag rules sort before PR rules; tag rules alphabetical among themselves.
- Tag rules have no in-progress exemption.
- An active `/` filter bypasses every hide rule (unchanged).
- Call the feature "workspace tags" in user-facing text; "prompt tags" is a different feature.
- No Claude attribution in commits. Terse comments (≤ 2–4 lines). Commit locally only; never push.
- Each task ends with `cargo test` and `cargo clippy --all-targets -- -D warnings` green.

## Review Focus

1. **Tag named like a PR label** (`#merged` tag rule plus `pr:merged` rule on the same workspace): the workspace must appear once, in the tag group. → Task 2 test `tag_rule_wins_over_pr_rule_in_dashboard`.
2. **Rule for a tag no workspace has anymore**: picker must still list it so it can be turned off; dashboard must not draw an empty group. → Task 6 test `stale_tag_rule_is_listed_with_zero_count`; Task 2 relies on existing "empty groups omitted".
3. **Uppercase / spaced input** (`" Parked "` in the editor or CLI): stored as `parked`, never as a duplicate. → Task 1 `normalize_tag` tests; Task 5 test `enter_adds_normalized_tag`.
4. **Narrow row with long tags**: the branch name must never be cut to make room for tags. → Task 4 test `tags_never_squeeze_the_branch_name`.
5. **Workspace deleted/archived with tags**: no orphan rows, `all_tags` forgets the tag. → Task 1 test `tags_cascade_on_workspace_delete`.

---

## File Structure

- Create `src/data/tags.rs` — `normalize_tag`, `TagEdit` helpers used by CLI and TUI.
- Modify `src/data/mod.rs` — `pub mod tags;`.
- Modify `src/data/schema.rs` — v30 `workspace_tags` table.
- Modify `src/data/store.rs` — `Workspace.tags`, loaders, `add_workspace_tag`, `remove_workspace_tag`, `all_tags`.
- Modify `src/data/hide_rules.rs` — `HideRule::Tag`, drop `Copy`, `PR_RULES`, `HideSubject.tags`.
- Modify `src/app/types.rs` — `SelectionTarget` drops `Copy`.
- Modify `src/app/selection.rs`, `src/app/render/dashboard.rs`, `src/app/input/dashboard.rs`, `src/ui/dashboard/{mod,hidden,by_repo,by_attention}.rs` — clone instead of copy; tags into subject and row.
- Modify `src/ui/dashboard/row.rs` — `RowInputs.tags`, `fit_tags`, render.
- Modify `src/ui/dashboard/mod.rs` — `matches_filter` tags.
- Create `src/ui/modal/workspace_tags.rs` — pure list builder for the editor.
- Modify `src/ui/modal/mod.rs` — `Modal::WorkspaceTags`, `Modal::HideRules.rows`, render text, actions card line.
- Modify `src/app/input/modal/{mod,pickers,workspace}.rs` — editor input, picker input, `#` forwarding.
- Modify `src/cli/{action,groups,run,tests}.rs`, `src/cli/parse/workspace.rs` — `workspace tag`.
- Modify `src/commands/inspect.rs` — `WorkspaceRecord.tags`.
- Tests: `src/app/input/tests/hide_rules.rs`, new `src/app/input/tests/workspace_tags.rs` (+ register in `src/app/input/tests/mod.rs`), `src/ui/dashboard/tests.rs`, inline `#[cfg(test)]` modules.
- Docs: `docs/book/src/daily-use/workspace-tags.md` (new), `hiding-workspaces.md`, `keybindings.md`, `SUMMARY.md`, `cli-reference/workspace-management.md`.

---

### Task 1: Tag names, schema v30, store

**Files:**
- Create: `src/data/tags.rs`
- Modify: `src/data/mod.rs`, `src/data/schema.rs:223-227`, `src/data/store.rs:134-150,296-420`
- Test: inline in `src/data/tags.rs` and `src/data/store.rs` tests module

**Interfaces:**
- Produces:
  - `pub fn normalize_tag(raw: &str) -> Option<String>`
  - `pub const MAX_TAG_LEN: usize = 32;`
  - `Workspace.tags: Vec<std::sync::Arc<str>>` (sorted)
  - `Store::add_workspace_tag(&self, id: WorkspaceId, tag: &str) -> Result<()>`
  - `Store::remove_workspace_tag(&self, id: WorkspaceId, tag: &str) -> Result<()>`
  - `Store::all_tags(&self) -> Result<Vec<(String, usize)>>` (sorted by name)

- [ ] **Step 1: Write failing `normalize_tag` tests** in new `src/data/tags.rs`:

```rust
//! Workspace tag names: one normalizer shared by the TUI and CLI.

pub const MAX_TAG_LEN: usize = 32;

/// Trimmed, lowercased `[a-z0-9_-]{1,32}`, or `None`. Keeps tags safe
/// inside the comma-separated `tag:<name>` hide-rule token.
pub fn normalize_tag(raw: &str) -> Option<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_lowercases() {
        assert_eq!(normalize_tag("  Parked ").as_deref(), Some("parked"));
    }

    #[test]
    fn accepts_digits_dash_underscore() {
        assert_eq!(normalize_tag("q4_spike-2").as_deref(), Some("q4_spike-2"));
    }

    #[test]
    fn rejects_empty_and_bad_chars() {
        for bad in ["", "   ", "a b", "a,b", "a:b", "#a", "ünï"] {
            assert_eq!(normalize_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn enforces_max_len() {
        assert!(normalize_tag(&"a".repeat(MAX_TAG_LEN)).is_some());
        assert!(normalize_tag(&"a".repeat(MAX_TAG_LEN + 1)).is_none());
    }
}
```

Add `pub mod tags;` to `src/data/mod.rs`.

- [ ] **Step 2: Run** `cargo test data::tags` — expect FAIL (panics at `todo!`).

- [ ] **Step 3: Implement**

```rust
pub fn normalize_tag(raw: &str) -> Option<String> {
    let t = raw.trim().to_lowercase();
    let ok = !t.is_empty()
        && t.len() <= MAX_TAG_LEN
        && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    ok.then_some(t)
}
```

- [ ] **Step 4: Run** `cargo test data::tags` — PASS.

- [ ] **Step 5: Write failing store tests** in `src/data/store.rs` `mod tests`:

```rust
    fn ws_in(store: &Store, repo: RepoId, name: &str) -> WorkspaceId {
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name,
                branch: name,
                worktree_path: Path::new(&format!("/tmp/tags-{name}")),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap()
    }

    #[test]
    fn workspace_tags_load_sorted_and_add_is_idempotent() {
        let store = Store::open_in_memory().unwrap();
        let r = store.add_repo(Path::new("/r"), "r", "").unwrap();
        let w = ws_in(&store, r, "a");
        store.add_workspace_tag(w, "spike").unwrap();
        store.add_workspace_tag(w, "parked").unwrap();
        store.add_workspace_tag(w, "spike").unwrap();
        let tags: Vec<String> = store.workspace_by_id(w).unwrap().unwrap()
            .tags.iter().map(|t| t.to_string()).collect();
        assert_eq!(tags, ["parked", "spike"]);
        assert_eq!(store.workspaces(r).unwrap()[0].tags.len(), 2);
        assert_eq!(store.all_workspaces().unwrap()[0].tags.len(), 2);
    }

    #[test]
    fn remove_workspace_tag_is_idempotent() {
        let store = Store::open_in_memory().unwrap();
        let r = store.add_repo(Path::new("/r"), "r", "").unwrap();
        let w = ws_in(&store, r, "a");
        store.add_workspace_tag(w, "spike").unwrap();
        store.remove_workspace_tag(w, "spike").unwrap();
        store.remove_workspace_tag(w, "spike").unwrap();
        assert!(store.workspace_by_id(w).unwrap().unwrap().tags.is_empty());
    }

    #[test]
    fn all_tags_are_distinct_sorted_and_counted() {
        let store = Store::open_in_memory().unwrap();
        let r1 = store.add_repo(Path::new("/r1"), "r1", "").unwrap();
        let r2 = store.add_repo(Path::new("/r2"), "r2", "").unwrap();
        let a = ws_in(&store, r1, "a");
        let b = ws_in(&store, r2, "b");
        store.add_workspace_tag(a, "spike").unwrap();
        store.add_workspace_tag(b, "spike").unwrap();
        store.add_workspace_tag(b, "parked").unwrap();
        assert_eq!(
            store.all_tags().unwrap(),
            vec![("parked".to_string(), 1), ("spike".to_string(), 2)]
        );
    }

    #[test]
    fn tags_cascade_on_workspace_delete() {
        let store = Store::open_in_memory().unwrap();
        let r = store.add_repo(Path::new("/r"), "r", "").unwrap();
        let w = ws_in(&store, r, "a");
        store.add_workspace_tag(w, "spike").unwrap();
        store.delete_workspace(w).unwrap();
        assert!(store.all_tags().unwrap().is_empty());
    }

    #[test]
    fn v30_migration_reruns_cleanly() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store.migrate().unwrap();
        let v: i64 = store.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert!(v >= 30);
    }
```

(Use whatever `AgentKind` / `NewWorkspace` imports the module already has; add `use crate::pty::session::AgentKind;` if missing.)

- [ ] **Step 6: Run** `cargo test data::store` — FAIL (no `tags` field / methods).

- [ ] **Step 7: Implement schema v30.** In `src/data/schema.rs`, after the `v < 29` block:

```rust
        if v < 30 {
            self.conn().execute_batch(SCHEMA_V30_WORKSPACE_TAGS)?;
            self.conn().execute("PRAGMA user_version = 30", [])?;
        }
```

and next to `SCHEMA_V28_SHARE_REQUESTS`:

```rust
const SCHEMA_V30_WORKSPACE_TAGS: &str = "
CREATE TABLE IF NOT EXISTS workspace_tags (
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    tag          TEXT NOT NULL,
    PRIMARY KEY (workspace_id, tag)
);
";
```

- [ ] **Step 8: Implement store.** In `src/data/store.rs`:
  - Add to `Workspace`: `/// Sorted; see [[crate::data::tags]].` `pub tags: Vec<std::sync::Arc<str>>,`
  - Introduce one column list and use it in `workspaces`, `workspace_by_id`, `all_workspaces` (replacing the three duplicated `SELECT` lists):

```rust
/// Column 12 is the comma-joined tag list; tags can't contain commas.
const WORKSPACE_COLS: &str = "id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent, shared, name_color,
     (SELECT group_concat(tag, ',') FROM workspace_tags t WHERE t.workspace_id = workspaces.id)";
```

    e.g. `&format!("SELECT {WORKSPACE_COLS} FROM workspaces WHERE repo_id = ?1 ORDER BY id")`.
  - In `row_to_workspace`, append:

```rust
        tags: {
            let mut v: Vec<std::sync::Arc<str>> = r
                .get::<_, Option<String>>(12)?
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(std::sync::Arc::from)
                .collect();
            v.sort();
            v
        },
```

  - Methods next to `set_workspace_name_color`:

```rust
    pub fn add_workspace_tag(&self, id: WorkspaceId, tag: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO workspace_tags (workspace_id, tag) VALUES (?1, ?2)",
            rusqlite::params![id.0, tag],
        )?;
        Ok(())
    }

    pub fn remove_workspace_tag(&self, id: WorkspaceId, tag: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM workspace_tags WHERE workspace_id = ?1 AND tag = ?2",
            rusqlite::params![id.0, tag],
        )?;
        Ok(())
    }

    /// Every tag in use, with how many workspaces carry it.
    pub fn all_tags(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT tag, count(*) FROM workspace_tags GROUP BY tag ORDER BY tag",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as usize)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
```

  - Fix any compile errors from other `Workspace { .. }` literals (e.g. `src/cli/tests.rs:2051`) by adding `tags: Vec::new()`.

- [ ] **Step 9: Run** `cargo test` — PASS. `cargo clippy --all-targets -- -D warnings` — clean.

- [ ] **Step 10: Commit**

```bash
git add src/data
git add -u
git commit -m "feat(data): store workspace tags (schema v30)"
```

---

### Task 2: Tag hide rules in the model and dashboard

**Files:**
- Modify: `src/data/hide_rules.rs` (whole file), `src/app/types.rs:17`, `src/app/selection.rs:36-110`, `src/app/render/dashboard.rs:85-100`, `src/app/input/dashboard.rs:385-430,655-670`, `src/app/input/modal/pickers.rs:110-150`, `src/ui/dashboard/{mod.rs,hidden.rs,by_repo.rs,by_attention.rs}`, `src/ui/modal/mod.rs:171-178,510-530`
- Test: `src/data/hide_rules.rs` tests, `src/ui/dashboard/tests.rs` `mod hide_rules`, `src/app/input/tests/hide_rules.rs`

**Interfaces:**
- Consumes: `Workspace.tags`, `normalize_tag` (Task 1).
- Produces:
  - `pub enum HideRule { Tag(Arc<str>), Pr(PrCriterion) }` — `Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord` (no `Copy`).
  - `HideRule::PR_RULES: [HideRule; 6]` (renamed from `ALL`).
  - `HideRule::tag(name: &str) -> HideRule` (caller passes a normalized name).
  - `HideRule::token(&self) -> Cow<'static, str>`, `label(&self) -> Cow<'static, str>`, `lifecycle(&self) -> Option<BranchLifecycle>`, `matches(&self, &HideSubject) -> bool`.
  - `HideRules::contains(&self, &HideRule)`, `toggle(&mut self, HideRule)`, `iter(&self) -> impl Iterator<Item = &HideRule>`, `classify(&self, &HideSubject) -> Option<HideRule>` (cloned).
  - `HideSubject.tags: Vec<Arc<str>>`; `HideSubject: Clone + Default` (no `Copy`).
  - `SelectionTarget`: `Clone, Debug, PartialEq, Eq` (no `Copy`).

- [ ] **Step 1: Write failing model tests** — add to `src/data/hide_rules.rs` tests:

```rust
    fn tagged(tags: &[&str]) -> HideSubject {
        HideSubject {
            lifecycle: Some(BranchLifecycle::PrMerged),
            tags: tags.iter().map(|t| std::sync::Arc::from(*t)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn tag_token_round_trips() {
        let r = HideRule::tag("parked");
        assert_eq!(r.token(), "tag:parked");
        assert_eq!(HideRule::from_token("tag:parked"), Some(r));
    }

    #[test]
    fn invalid_tag_tokens_are_skipped() {
        let rules = HideRules::parse(Some("tag:Bad Name,tag:,tag:ok"));
        assert_eq!(rules.serialize().as_deref(), Some("tag:ok"));
    }

    #[test]
    fn tag_rules_sort_before_pr_rules_alphabetically() {
        let rules = HideRules::parse(Some("pr:merged,tag:zeta,tag:alpha"));
        assert_eq!(rules.serialize().as_deref(), Some("tag:alpha,tag:zeta,pr:merged"));
    }

    #[test]
    fn tag_rule_matches_only_tagged_workspaces_and_ignores_activity() {
        let r = HideRule::tag("parked");
        let busy = HideSubject { work_in_progress: true, agent_live: true, ..tagged(&["parked"]) };
        assert!(r.matches(&busy));
        assert!(!r.matches(&tagged(&["spike"])));
    }

    #[test]
    fn classify_prefers_tag_rule_over_pr_rule() {
        let rules = HideRules::parse(Some("pr:merged,tag:parked"));
        assert_eq!(rules.classify(&tagged(&["parked"])), Some(HideRule::tag("parked")));
    }

    #[test]
    fn tag_label_and_lifecycle() {
        let r = HideRule::tag("parked");
        assert_eq!(r.label(), "#parked");
        assert_eq!(r.lifecycle(), None);
    }
```

Rename `HideRule::ALL` to `HideRule::PR_RULES` in existing tests (`tokens_round_trip`, `none_lifecycle_never_matches`). `other_pr_rules_ignore_activity` builds a full `HideSubject` literal; add `..Default::default()` or `tags: vec![]`.

- [ ] **Step 2: Run** `cargo test data::hide_rules` — FAIL to compile.

- [ ] **Step 3: Implement the model** in `src/data/hide_rules.rs`:

```rust
use std::borrow::Cow;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HideRule {
    /// Declared first: derived `Ord` puts tag rules ahead of PR rules.
    Tag(Arc<str>),
    Pr(PrCriterion),
}

#[derive(Debug, Clone, Default)]
pub struct HideSubject {
    pub lifecycle: Option<BranchLifecycle>,
    pub work_in_progress: bool,
    pub agent_live: bool,
    pub idle_secs: Option<u64>,
    /// The workspace's tags, sorted.
    pub tags: Vec<Arc<str>>,
}

impl HideRule {
    pub const PR_RULES: [HideRule; 6] = [ /* same six as before */ ];

    pub fn tag(name: &str) -> HideRule {
        HideRule::Tag(Arc::from(name))
    }

    pub fn token(&self) -> Cow<'static, str> {
        match self {
            HideRule::Tag(t) => Cow::Owned(format!("tag:{t}")),
            HideRule::Pr(c) => Cow::Borrowed(match c { /* existing pr:* arms */ }),
        }
    }

    pub fn from_token(s: &str) -> Option<HideRule> {
        if let Some(name) = s.strip_prefix("tag:") {
            return crate::data::tags::normalize_tag(name)
                .filter(|n| n == name)
                .map(|n| HideRule::tag(&n));
        }
        HideRule::PR_RULES.into_iter().find(|r| r.token() == s)
    }

    pub fn label(&self) -> Cow<'static, str> {
        match self {
            HideRule::Tag(t) => Cow::Owned(format!("#{t}")),
            HideRule::Pr(c) => Cow::Borrowed(match c { /* existing labels */ }),
        }
    }

    pub fn lifecycle(&self) -> Option<BranchLifecycle> {
        let HideRule::Pr(c) = self else { return None };
        Some(match c { /* existing arms */ })
    }

    /// Tag rules hide regardless of activity: tagging is deliberate.
    pub fn matches(&self, s: &HideSubject) -> bool {
        match self {
            HideRule::Tag(t) => s.tags.contains(t),
            HideRule::Pr(PrCriterion::NoPr) => {
                s.lifecycle == Some(BranchLifecycle::NoPr) && !s.in_progress()
            }
            HideRule::Pr(_) => s.lifecycle.is_some() && s.lifecycle == self.lifecycle(),
        }
    }
}
```

`HideRules`: `contains(&self, rule: &HideRule)`, `toggle` compares by `==` with a moved rule, `iter()` returns `self.0.iter()`, `classify` returns `self.iter().find(|r| r.matches(s)).cloned()`. `serialize` maps `r.token()` then joins.

- [ ] **Step 4: Run** `cargo test data::hide_rules` — PASS for that module (crate may not compile yet; use `cargo test --lib data::hide_rules` after Step 5 if needed).

- [ ] **Step 5: Drop `Copy` downstream (compiler-driven).**
  - `src/app/types.rs:17`: `#[derive(Debug, Clone, PartialEq, Eq)]` on `SelectionTarget`.
  - `App::selected_target` → `self.dashboard.selection.clone()`.
  - `select_index` → `self.selectable.get(idx).cloned()`.
  - `src/ui/dashboard/mod.rs` reconcile: `.position(|s| *s == t)` stays; `.copied()` → `.cloned()`; pass `&t` where `target_exists` takes it by value (clone if needed).
  - `hidden_groups`: `.filter(|w| ... && hidden_by(w, filter).as_ref() == Some(rule))` with `rule: &HideRule`; return `(rule.clone(), rows)`.
  - `push_group_targets` / `hidden_group_view`: `SelectionTarget::HiddenGroup { repo, rule: rule.clone() }`; `expanded_hidden.contains(&(repo, rule.clone()))`.
  - `src/app/input/dashboard.rs:391-430`: bind `rule` by value from `app.selected_target()` (already owned) and clone where used twice.
  - `src/app/render/dashboard.rs:89`: `if let Some(SelectionTarget::Workspace(id)) = app.dashboard.selection.clone()`.
  - `src/ui/dashboard/hidden.rs`: tag rules have no lifecycle and their label already starts with `#`, so skip the glyph span (`▸ #parked  ·  2 hidden`):

```rust
    if lc.is_some() {
        spans.push(Span::styled(
            format!("{} ", crate::ui::theme::branch_glyph(lc, nerd_fonts)),
            theme.lifecycle_style(lc).unwrap_or_else(|| theme.dim_style()),
        ));
    }
```

  - `src/app/input/dashboard.rs:659` and `src/app/input/modal/pickers.rs:122,131`, `src/ui/modal/mod.rs:517`: temporarily rename `ALL` → `PR_RULES` and use `.clone()`/`&rule`; Task 6 replaces this code.
  - In tests: `const MERGED: HideRule = HideRule::Pr(..)` still compiles; add `.clone()` where a `SelectionTarget`/`HideRule` is used after move.

- [ ] **Step 6: Fill `HideSubject.tags`.** In `App::hide_subject_with_age` (`src/app/selection.rs`):

```rust
        let tags = self
            .workspaces
            .iter()
            .find(|(_, w)| w.id == ws)
            .map(|(_, w)| w.tags.clone())
            .unwrap_or_default();
        crate::data::hide_rules::HideSubject {
            lifecycle: self.pr_lifecycle.get(&ws).copied(),
            work_in_progress: dirty || ahead,
            agent_live: session_live || reported_live,
            idle_secs,
            tags,
        }
```

Run `grep -rn "HideSubject {" src` and confirm every construction site either sets `tags` or uses `..Default::default()`.

- [ ] **Step 7: Write failing dashboard tests** in `src/ui/dashboard/tests.rs` `mod hide_rules`. Add a helper and tests:

```rust
    fn tag(mut it: WorkspaceItem<'_>, t: &str) -> WorkspaceItem<'_> {
        it.hide.tags.push(std::sync::Arc::from(t));
        it
    }

    #[test]
    fn tag_rule_hides_into_a_hash_group() {
        let repo = repo_with("tag:parked");
        let mut ws = items(&repo);
        ws[0] = tag(item(&repo, 1, "open-pr", Some(PrOpen)), "parked");
        let screen = draw(&inputs(&repo, ws), &mut DashboardState::default());
        assert!(!screen.contains("open-pr"));
        assert!(line_with(&screen, "#parked").contains("1 hidden"));
    }

    #[test]
    fn tag_rule_wins_over_pr_rule_in_dashboard() {
        let repo = repo_with("pr:merged,tag:merged");
        let mut ws = items(&repo);
        ws[1] = tag(item(&repo, 2, "merged-a", Some(PrMerged)), "merged");
        let t = visible_targets(&inputs(&repo, ws), &DashboardState::default());
        let rid = RepoId(1);
        let tag_group = SelectionTarget::HiddenGroup { repo: rid, rule: HideRule::tag("merged") };
        let pr_group = SelectionTarget::HiddenGroup { repo: rid, rule: MERGED };
        let ti = t.iter().position(|x| *x == tag_group).expect("tag group");
        let pi = t.iter().position(|x| *x == pr_group).expect("pr group");
        assert!(ti < pi, "tag groups render first: {t:?}");
        let mut ws = items(&repo);
        ws[1] = tag(item(&repo, 2, "merged-a", Some(PrMerged)), "merged");
        let screen = draw(&inputs(&repo, ws), &mut DashboardState::default());
        assert!(screen.lines().any(|l| l.contains("#merged  ·  1 hidden")), "{screen}");
        assert!(
            screen.lines().any(|l| l.contains("merged  ·  1 hidden") && !l.contains('#')),
            "merged-b stays in the PR group: {screen}"
        );
    }
```

In `src/app/input/tests/hide_rules.rs` add an end-to-end test that tags flow from the store:

```rust
#[tokio::test]
async fn tagged_workspace_hides_under_tag_rule() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let repo = app.repos[0].id;
    let ids: Vec<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    app.store.add_workspace_tag(ids[0], "parked").unwrap();
    app.store
        .set_repo_hide_rules(repo, &HideRules::parse(Some("tag:parked")))
        .unwrap();
    app.refresh().unwrap();
    draw(&mut app);
    assert!(!app.selectable.contains(&SelectionTarget::Workspace(ids[0])));
    assert_eq!(app.hidden_group_of(ids[0]), Some((repo, HideRule::tag("parked"))));
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[1])));
}
```

- [ ] **Step 8: Run** `cargo test hide_rules` — PASS.

- [ ] **Step 9: Run** `cargo test && cargo clippy --all-targets -- -D warnings` — green.

- [ ] **Step 10: Commit**

```bash
git add -u
git commit -m "feat(dashboard): hide workspaces by tag"
```

---

### Task 3: `wsx workspace tag` CLI

**Files:**
- Modify: `src/data/tags.rs`, `src/cli/action.rs`, `src/cli/parse/workspace.rs`, `src/cli/run.rs`, `src/cli/groups.rs:25-50`, `src/commands/inspect.rs:114-125,185-230`
- Test: `src/cli/tests.rs`, `src/data/tags.rs`, `src/commands/inspect.rs`

**Interfaces:**
- Consumes: `normalize_tag`, store tag methods (Task 1).
- Produces:
  - `pub fn normalize_all(raw: &[String]) -> crate::error::Result<Vec<String>>` in `data::tags` — `Err(Error::UserInput("invalid tag '<raw>': use 1-32 of a-z 0-9 - _"))` on the first bad one; deduped, input order.
  - `CliAction::WorkspaceTag { op: TagOp, tags: Vec<String>, workspace: Option<String>, all: bool, json: bool }`
  - `pub enum TagOp { Add, Rm, List }` in `src/cli/action.rs`.
  - `WorkspaceRecord.tags: Vec<String>`.

- [ ] **Step 1: Failing tests.** In `src/data/tags.rs`:

```rust
    #[test]
    fn normalize_all_rejects_whole_batch_on_one_bad_tag() {
        let err = normalize_all(&["ok".into(), "bad tag".into()]).unwrap_err();
        assert!(err.to_string().contains("bad tag"), "{err}");
    }

    #[test]
    fn normalize_all_dedupes_after_normalizing() {
        assert_eq!(normalize_all(&["A".into(), "a".into(), "b".into()]).unwrap(), ["a", "b"]);
    }
```

In `src/cli/tests.rs`:

```rust
#[test]
fn workspace_tag_add_parses_tags_and_workspace_flag() {
    match parse(&["workspace", "tag", "add", "parked", "spike", "--workspace", "r/s"]).unwrap() {
        CliAction::WorkspaceTag { op: TagOp::Add, tags, workspace, .. } => {
            assert_eq!(tags, ["parked", "spike"]);
            assert_eq!(workspace.as_deref(), Some("r/s"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn workspace_tag_add_needs_a_tag() {
    assert!(parse(&["workspace", "tag", "add"]).is_err());
}

#[test]
fn workspace_tag_list_all_and_json() {
    match parse(&["workspace", "tag", "list", "--all", "--json"]).unwrap() {
        CliAction::WorkspaceTag { op: TagOp::List, all: true, json: true, .. } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn workspace_tag_list_rejects_all_with_workspace() {
    assert!(parse(&["workspace", "tag", "list", "--all", "--workspace", "r/s"]).is_err());
}
```

In `src/commands/inspect.rs` tests, extend `workspace_records_carry_the_whole_dashboard_row` (or add a sibling) asserting `recs[0].tags == ["parked"]` after `store.add_workspace_tag(id, "parked")`.

- [ ] **Step 2: Run** `cargo test cli::tests::workspace_tag data::tags inspect` — FAIL.

- [ ] **Step 3: Implement.**
  - `src/data/tags.rs`:

```rust
/// Normalize every tag or none: a CLI batch with one typo writes nothing.
pub fn normalize_all(raw: &[String]) -> crate::error::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for r in raw {
        let t = normalize_tag(r).ok_or_else(|| {
            crate::error::Error::UserInput(format!(
                "invalid tag '{r}': use 1-{MAX_TAG_LEN} of a-z 0-9 - _"
            ))
        })?;
        if !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}
```

  - `src/cli/action.rs`: add `TagOp` and the `WorkspaceTag` variant (doc: `/// \`<repo>/<slug>\`; \`None\` = current workspace.` on `workspace`).
  - `src/cli/parse/workspace.rs`, new arm before `share`:

```rust
        Some("tag") => {
            const USAGE: &str = "workspace tag add|rm <tag>... [--workspace <repo>/<slug>] | list [--workspace <repo>/<slug> | --all] [--json]";
            let usage = || Error::Usage { group: None, msg: USAGE.into() };
            let op = match it.next().as_deref() {
                Some("add") => TagOp::Add,
                Some("rm") => TagOp::Rm,
                Some("list") => TagOp::List,
                _ => return Err(usage()),
            };
            let (mut tags, mut workspace, mut all, mut json) = (Vec::new(), None, false, false);
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--workspace" => {
                        workspace = Some(it.next().filter(|v| !v.starts_with("--")).ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--workspace needs value (<repo>/<slug>)".into(),
                        })?)
                    }
                    "--all" if op == TagOp::List => all = true,
                    "--json" if op == TagOp::List => json = true,
                    s if s.starts_with("--") => return Err(usage()),
                    _ if op != TagOp::List => tags.push(arg),
                    _ => return Err(usage()),
                }
            }
            if (op != TagOp::List && tags.is_empty()) || (all && workspace.is_some()) {
                return Err(usage());
            }
            Ok(CliAction::WorkspaceTag { op, tags, workspace, all, json })
        }
```

    (`TagOp` derives `Debug, Clone, Copy, PartialEq, Eq`; import it with `use crate::cli::action::TagOp;` in the parser and in `src/cli/tests.rs`.)
  - `src/cli/run.rs`:

```rust
        CliAction::WorkspaceTag { op, tags, workspace, all, json } => {
            if all {
                let rows = store.all_tags()?;
                if json {
                    let v: Vec<_> = rows.iter().map(|(t, n)| serde_json::json!({"tag": t, "workspaces": n})).collect();
                    println!("{}", serde_json::to_string_pretty(&v)?);
                } else {
                    for (t, n) in rows {
                        println!("{t}\t{n}");
                    }
                }
                return Ok(());
            }
            let ws = target_workspace(&store, workspace.as_deref())?;
            match op {
                TagOp::Add => {
                    for t in crate::data::tags::normalize_all(&tags)? {
                        store.add_workspace_tag(ws.id, &t)?;
                    }
                }
                TagOp::Rm => {
                    for t in crate::data::tags::normalize_all(&tags)? {
                        store.remove_workspace_tag(ws.id, &t)?;
                    }
                }
                TagOp::List => {}
            }
            let now = store.workspace_by_id(ws.id)?.map(|w| w.tags).unwrap_or_default();
            if json {
                let v: Vec<&str> = now.iter().map(|t| &**t).collect();
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                for t in now {
                    println!("{t}");
                }
            }
        }
```

    (`add`/`rm` print the resulting tag list so the caller sees the outcome.)
  - `src/cli/groups.rs` workspace group, after `unshare`:

```rust
            CmdInfo {
                usage: "tag add|rm <tag>... [--workspace <repo>/<slug>]",
                blurb: "Add or remove workspace tags (current workspace by default)",
            },
            CmdInfo {
                usage: "tag list [--workspace <repo>/<slug> | --all] [--json]",
                blurb: "List a workspace's tags, or every tag in use with counts",
            },
```

  - `src/commands/inspect.rs`: `pub tags: Vec<String>,` on `WorkspaceRecord` (additive) and `tags: w.tags.iter().map(|t| t.to_string()).collect(),` in `workspace_records`.

- [ ] **Step 4: Run** `cargo test` — PASS (fix any help-snapshot test in `src/cli/tests.rs` that lists workspace commands by appending the two new lines). `cargo clippy --all-targets -- -D warnings` — clean.

- [ ] **Step 5: Smoke** `cargo run -q -- workspace tag list --all` — prints nothing or existing tags; exit 0.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(cli): wsx workspace tag add/rm/list"
```

---

### Task 4: Tags on dashboard rows and in the `/` filter

**Files:**
- Modify: `src/ui/dashboard/row.rs:171-220,386-450`, `src/app/render/dashboard.rs:382-430`, `src/ui/dashboard/mod.rs:809-831`, every `RowInputs { .. }` literal (`grep -rn "has_multi_pane_layout:" src`)
- Test: `src/ui/dashboard/row.rs` tests, `src/ui/dashboard/tests.rs`

**Interfaces:**
- Consumes: `Workspace.tags`.
- Produces: `RowInputs.tags: Vec<Arc<str>>`; `fn fit_tags(tags: &[Arc<str>], room: usize) -> String` (private to `row.rs`).

- [ ] **Step 1: Failing tests** in `row.rs` tests (`base()` gains `tags: Vec::new()`):

```rust
    fn with_tags(tags: &[&str]) -> RowInputs {
        let mut i = base();
        i.tags = tags.iter().map(|t| std::sync::Arc::from(*t)).collect();
        i
    }

    #[test]
    fn fit_tags_keeps_whole_tags_and_counts_the_rest() {
        let t: Vec<std::sync::Arc<str>> = ["a", "bb", "ccc"].iter().map(|s| (*s).into()).collect();
        assert_eq!(fit_tags(&t, 100), " #a #bb #ccc");
        assert_eq!(fit_tags(&t, 10), " #a #bb +1");
        assert_eq!(fit_tags(&t, 9), " #a +2");
        assert_eq!(fit_tags(&t, 3), " +3");
        assert_eq!(fit_tags(&t, 2), "");
        assert_eq!(fit_tags(&[], 100), "");
    }

    #[test]
    fn tags_render_dim_after_the_branch() {
        let theme = Theme::wsx();
        let line = render(&with_tags(&["parked"]), ColumnWidths::default(), 0, &theme, 160);
        let text = line_text(&line);
        let b = text.find("bakedbean/repo-overview").unwrap();
        let t = text.find("#parked").expect("tag drawn");
        assert!(t > b);
        let span = line.spans.iter().find(|s| s.content.contains("#parked")).unwrap();
        assert_eq!(span.style.fg, theme.dim_style().fg);
    }

    #[test]
    fn tags_never_squeeze_the_branch_name() {
        let theme = Theme::wsx();
        let widths = ColumnWidths { branch: 26, ..ColumnWidths::default() };
        let text = line_text(&render(&with_tags(&["parked", "spike"]), widths, 0, &theme, 160));
        assert!(text.contains("bakedbean/repo-overview"), "{text:?}");
        assert!(!text.contains("#parked"), "no room for tags: {text:?}");
    }

    #[test]
    fn tagged_row_keeps_column_alignment() {
        let theme = Theme::wsx();
        let w = ColumnWidths::default();
        let plain = display_width(&line_text(&render(&base(), w, 0, &theme, 160)));
        let tagged = display_width(&line_text(&render(&with_tags(&["a", "b"]), w, 0, &theme, 160)));
        assert_eq!(plain, tagged);
    }
```

(If `ColumnWidths` has no struct-update-friendly `branch` field name, match its actual field; it's used as `widths.branch` in `render`.)

Filter tests in `src/ui/dashboard/tests.rs` next to `matches_filter_matches_fallback_text_and_branch` (reuse that test's item builder; set `row.tags`):

```rust
#[test]
fn matches_filter_matches_tags_by_substring_and_hash_exactly() {
    // build `w` as in matches_filter_matches_fallback_text_and_branch, then:
    // w.row.tags = vec!["parked".into()]; w.row.branch = "spike-x".into();
    assert!(matches_filter(&w, "park"));
    assert!(matches_filter(&w, "#parked"));
    assert!(!matches_filter(&w, "#park"), "# is exact");
    assert!(!matches_filter(&w, "#spike"), "# skips branch");
    assert!(matches_filter(&w, "spike"), "plain still hits branch");
}
```

- [ ] **Step 2: Run** `cargo test dashboard` — FAIL.

- [ ] **Step 3: Implement.**
  - `RowInputs`: `/// Workspace tags, sorted; drawn dim after the branch.` `pub tags: Vec<std::sync::Arc<str>>,`. Add `tags: Vec::new()` to every literal; in `src/app/render/dashboard.rs` builder: `tags: ws.tags.clone(),`.
  - `fit_tags`:

```rust
/// ` #a #b` within `room` cells; drops trailing tags behind ` +N`.
fn fit_tags(tags: &[std::sync::Arc<str>], room: usize) -> String {
    let mut out = String::new();
    for (i, t) in tags.iter().enumerate() {
        let next = format!(" #{t}");
        let rest = tags.len() - i - 1;
        let tail = if rest > 0 { format!(" +{rest}").len() } else { 0 };
        if display_width(&out) + display_width(&next) + tail > room {
            let more = format!(" +{}", tags.len() - i);
            return if display_width(&out) + more.len() <= room { out + &more } else { String::new() };
        }
        out.push_str(&next);
    }
    out
}
```

  - In `render`, after computing `branch_truncated`/`branch_visible_width` and before pushing badges: tags only get leftover room after the full name.

```rust
    let name_fits = display_width(&branch_text) <= branch_target;
    let tags_cell = if name_fits {
        fit_tags(&inputs.tags, branch_target - branch_visible_width)
    } else {
        String::new()
    };
    let tags_width = display_width(&tags_cell);
```

    Push `Span::styled(tags_cell, theme.dim_style())` right after the name span (before the setup/mail badges), and add `tags_width` to `consumed`.
  - `matches_filter` (`src/ui/dashboard/mod.rs`), at the top:

```rust
    if let Some(tag) = filter.strip_prefix('#') {
        let tag = tag.to_lowercase();
        return w.row.tags.iter().any(|t| **t == *tag);
    }
```

    and after the branch/repo check: `if w.row.tags.iter().any(|t| t.contains(&needle)) { return true; }`. Update the doc comment to mention tags and `#`.

- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings` — green.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(dashboard): show workspace tags on rows and match them in /"
```

---

### Task 5: Tag editor modal (`#`)

**Files:**
- Create: `src/ui/modal/workspace_tags.rs`, `src/app/input/tests/workspace_tags.rs`
- Modify: `src/ui/modal/mod.rs` (variant, render arm, actions card text, `mod workspace_tags;`), `src/app/input/modal/mod.rs` (dispatch), `src/app/input/modal/pickers.rs` (handler), `src/app/input/modal/workspace.rs:383-396` (forward `#`), `src/app/input/dashboard.rs` (`#` key), `src/app/input/tests/mod.rs` (register module)

**Interfaces:**
- Consumes: `normalize_tag`, store tag methods, `Workspace.tags`.
- Produces:
  - `Modal::WorkspaceTags { workspace_id: WorkspaceId, name: String, known: Vec<String>, tags: Vec<String>, input: String, selected: usize, notice: Option<String> }`
  - `pub struct TagItem { pub name: String, pub checked: bool, pub new: bool }`
  - `pub fn tag_items(known: &[String], tags: &[String], input: &str) -> Vec<TagItem>` in `src/ui/modal/workspace_tags.rs`.

- [ ] **Step 1: Failing pure tests** in `src/ui/modal/workspace_tags.rs`:

```rust
//! Rows for the `#` workspace-tags editor.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagItem {
    pub name: String,
    pub checked: bool,
    pub new: bool,
}

/// `known` ∪ `tags`, sorted, narrowed by `input`; a valid unseen `input`
/// leads as a `(new)` row.
pub fn tag_items(known: &[String], tags: &[String], input: &str) -> Vec<TagItem> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn lists_known_and_checks_current() {
        let items = tag_items(&s(&["parked", "ui"]), &s(&["ui"]), "");
        assert_eq!(items.iter().map(|i| (i.name.as_str(), i.checked)).collect::<Vec<_>>(),
                   [("parked", false), ("ui", true)]);
    }

    #[test]
    fn filters_by_substring_and_offers_new_first() {
        let items = tag_items(&s(&["parked", "spike"]), &[], "Sp");
        assert_eq!(items[0], TagItem { name: "sp".into(), checked: false, new: true });
        assert_eq!(items[1].name, "spike");
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn no_new_row_for_existing_or_invalid_input() {
        assert!(tag_items(&s(&["spike"]), &[], "spike").iter().all(|i| !i.new));
        assert!(tag_items(&[], &[], "a b").is_empty());
    }
}
```

- [ ] **Step 2: Run** `cargo test workspace_tags` — FAIL.

- [ ] **Step 3: Implement `tag_items`:**

```rust
pub fn tag_items(known: &[String], tags: &[String], input: &str) -> Vec<TagItem> {
    let needle = input.trim().to_lowercase();
    let mut names: Vec<&String> = known.iter().chain(tags).collect();
    names.sort();
    names.dedup();
    let mut out: Vec<TagItem> = Vec::new();
    if let Some(n) = crate::data::tags::normalize_tag(input) {
        if !names.iter().any(|k| **k == n) {
            out.push(TagItem { name: n, checked: false, new: true });
        }
    }
    out.extend(names.into_iter().filter(|k| k.contains(&needle)).map(|k| TagItem {
        name: k.clone(),
        checked: tags.contains(k),
        new: false,
    }));
    out
}
```

- [ ] **Step 4: Run** — PASS.

- [ ] **Step 5: Failing input tests** in new `src/app/input/tests/workspace_tags.rs` (register `mod workspace_tags;` in `src/app/input/tests/mod.rs`):

```rust
//! The `#` workspace-tags editor.

use super::common::*;
use super::*;
use crate::ui::modal::Modal;
use crossterm::event::KeyModifiers;

async fn open_editor(app: &mut App) -> crate::data::store::WorkspaceId {
    app.refresh().unwrap();
    let ws = app.workspaces[0].1.id;
    app.dashboard.selection = Some(SelectionTarget::Workspace(ws));
    press(app, '#', KeyModifiers::SHIFT).await;
    assert!(matches!(app.modal, Some(Modal::WorkspaceTags { .. })), "{:?}", app.modal);
    ws
}

async fn modal_key(app: &mut App, code: KeyCode) {
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(app, &shared, key(code)).await.unwrap();
}

async fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        modal_key(app, KeyCode::Char(c)).await;
    }
}

fn stored_tags(app: &App, ws: crate::data::store::WorkspaceId) -> Vec<String> {
    app.store.workspace_by_id(ws).unwrap().unwrap().tags.iter().map(|t| t.to_string()).collect()
}

#[tokio::test]
async fn enter_adds_normalized_tag_and_stays_open() {
    let mut app = test_app_with_two_ready_workspaces();
    let ws = open_editor(&mut app).await;
    type_str(&mut app, "Parked").await;
    modal_key(&mut app, KeyCode::Enter).await;
    assert_eq!(stored_tags(&app, ws), ["parked"]);
    let Some(Modal::WorkspaceTags { input, tags, .. }) = &app.modal else { panic!() };
    assert!(input.is_empty());
    assert_eq!(tags, &["parked"]);
}

#[tokio::test]
async fn invalid_input_shows_notice_and_writes_nothing() {
    let mut app = test_app_with_two_ready_workspaces();
    let ws = open_editor(&mut app).await;
    type_str(&mut app, "a:b").await;
    modal_key(&mut app, KeyCode::Enter).await;
    assert!(stored_tags(&app, ws).is_empty());
    let Some(Modal::WorkspaceTags { notice, .. }) = &app.modal else { panic!() };
    assert!(notice.is_some());
}

#[tokio::test]
async fn space_toggles_existing_tag_off() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let ws = app.workspaces[0].1.id;
    app.store.add_workspace_tag(ws, "parked").unwrap();
    open_editor(&mut app).await;
    modal_key(&mut app, KeyCode::Char(' ')).await;
    assert!(stored_tags(&app, ws).is_empty());
}

#[tokio::test]
async fn suggestions_include_tags_from_other_workspaces() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    app.store.add_workspace_tag(app.workspaces[1].1.id, "ui").unwrap();
    open_editor(&mut app).await;
    let Some(Modal::WorkspaceTags { known, .. }) = &app.modal else { panic!() };
    assert_eq!(known, &["ui"]);
}

#[tokio::test]
async fn esc_closes_and_empty_enter_closes() {
    let mut app = test_app_with_two_ready_workspaces();
    open_editor(&mut app).await;
    modal_key(&mut app, KeyCode::Enter).await;
    assert!(app.modal.is_none());
    open_editor(&mut app).await;
    modal_key(&mut app, KeyCode::Esc).await;
    assert!(app.modal.is_none());
}

#[tokio::test]
async fn hash_on_actions_card_opens_editor() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    app.dashboard.selection = Some(SelectionTarget::Workspace(app.workspaces[0].1.id));
    app.modal = Some(Modal::WorkspaceActions);
    modal_key(&mut app, KeyCode::Char('#')).await;
    assert!(matches!(app.modal, Some(Modal::WorkspaceTags { .. })));
}
```

(Use the helper names that exist in `src/app/input/tests/common.rs` — `press`, `key`, `shared_app` are used by `hide_rules.rs`.)

- [ ] **Step 6: Run** `cargo test input::tests::workspace_tags` — FAIL.

- [ ] **Step 7: Implement.**
  - `Modal::WorkspaceTags { .. }` variant in `src/ui/modal/mod.rs` with a 2-line doc: `/// The \`#\` tag editor. \`known\`/\`tags\` are snapshots refreshed after each write; \`selected\` indexes \`tag_items(..)\`.`
  - Render arm:

```rust
        Modal::WorkspaceTags { name, known, tags, input, selected, notice, .. } => {
            let items = workspace_tags::tag_items(known, tags, input);
            let list = items.iter().enumerate().map(|(i, it)| {
                let cursor = if i == *selected { ">" } else { " " };
                let mark = if it.checked { "x" } else { " " };
                let new = if it.new { "  (new)" } else { "" };
                format!("{cursor} [{mark}] {}{new}", it.name)
            }).collect::<Vec<_>>().join("\n");
            let notice = notice.as_deref().map(|n| format!("\n{n}")).unwrap_or_default();
            (
                "workspace tags",
                format!("{name}\n\n# {input}\u{2588}\n{list}{notice}\n\n\
                         \u{2191}\u{2193} move   space toggle   enter add typed   Esc close"),
            )
        }
```

  - Actions card: change the line `r   rename      C   name color` block to add `#   tags` (e.g. `o   setup log   x   cancel setup\n  #   tags\n\n`).
  - Opening helper in `src/app/input/modal/pickers.rs`:

```rust
/// Open (or reopen after a write) the `#` editor for `ws` with fresh snapshots.
pub(crate) fn open_workspace_tags(app: &mut App, ws: WorkspaceId, input: String, selected: usize, notice: Option<String>) -> Result<()> {
    let Some(w) = app.store.workspace_by_id(ws)? else { app.modal = None; return Ok(()) };
    let known = app.store.all_tags()?.into_iter().map(|(t, _)| t).collect();
    app.modal = Some(Modal::WorkspaceTags {
        workspace_id: ws,
        name: w.name,
        known,
        tags: w.tags.iter().map(|t| t.to_string()).collect(),
        input,
        selected,
        notice,
    });
    Ok(())
}
```

  - Handler `workspace_tags_editor(app, k, workspace_id, known, tags, input, selected)` in the same file:
    - `Esc` → `app.modal = None`.
    - `Enter` with `input.trim().is_empty()` → close.
    - `Enter` otherwise → `match normalize_tag(&input)`: `Some(t)` → `store.add_workspace_tag`, `app.refresh()?`, `open_workspace_tags(app, ws, String::new(), 0, None)`; `None` → reopen with same input and `notice: Some(format!("invalid tag: use 1-{MAX_TAG_LEN} of a-z 0-9 - _"))`.
    - `Char(' ')` → item = `tag_items(..)[selected]`; if `checked` remove else add; `app.refresh()?`; reopen with same input and selected.
    - `Up`/`Down` → move within `0..items.len()`, saturating.
    - `Backspace` → pop input, selected 0, clear notice.
    - `Char(c)` without CONTROL/ALT → push, selected 0, clear notice.
    - Store errors → `Modal::Error { message: format!("could not save tags: {e}") }`.
  - Dispatch arm in `src/app/input/modal/mod.rs`.
  - Dashboard key in `src/app/input/dashboard.rs` near `'C'`:

```rust
        (KeyCode::Char('#'), _) => {
            if let Some(SelectionTarget::Workspace(ws)) = app.selected_target() {
                crate::app::input::modal::pickers::open_workspace_tags(app, ws, String::new(), 0, None)?;
            }
        }
```

    (Adjust visibility: make `pickers` reachable or expose the helper via `crate::app::input::modal`.)
  - Actions card forwarding: add `| KeyCode::Char('#')` to the forwarded list at `src/app/input/modal/workspace.rs:388-393`.

- [ ] **Step 8: Run** `cargo test && cargo clippy --all-targets -- -D warnings` — green.

- [ ] **Step 9: Commit**

```bash
git add src/ui/modal/workspace_tags.rs src/app/input/tests/workspace_tags.rs
git add -u
git commit -m "feat(tui): # tag editor for workspaces"
```

---

### Task 6: Dynamic rows in the `F` hide-rules picker

**Files:**
- Modify: `src/ui/modal/mod.rs:171-178,510-530`, `src/app/input/dashboard.rs:655-672`, `src/app/input/modal/pickers.rs:110-150`, `src/app/input/modal/mod.rs:126-133`
- Test: `src/app/input/tests/hide_rules.rs`, `src/ui/modal/mod.rs` tests

**Interfaces:**
- Consumes: `HideRule::{tag, PR_RULES}`, `Store::all_tags`.
- Produces: `Modal::HideRules { repo_id, repo_name, rules, rows: Vec<(HideRule, usize)>, selected }`; `pub(crate) fn hide_rule_rows(app: &App, repo: RepoId, rules: &HideRules) -> Result<Vec<(HideRule, usize)>>` in `src/app/input/modal/pickers.rs`.

- [ ] **Step 1: Failing tests.** Update existing tests in `src/app/input/tests/hide_rules.rs`: replace `counts[0]` with `rows[0]` and `counts[2]` with lookups:

```rust
fn count_of(rows: &[(HideRule, usize)], rule: &HideRule) -> usize {
    rows.iter().find(|(r, _)| r == rule).map(|(_, n)| *n).expect("row present")
}
```

  - `f_opens_picker_for_selected_repo_with_counts`: `assert_eq!(count_of(rows, &MERGED), 1);`
  - `picker_counts_only_hideable_no_pr_workspaces`: `assert_eq!(count_of(rows, &NO_PR), 1);`
  - `space_toggles_and_persists`: unchanged (no tags → row 0 is still merged).

New tests:

```rust
#[tokio::test]
async fn picker_lists_global_tags_first_with_per_repo_counts() {
    let (mut app, _, ids) = app_with_merged_hidden();
    app.store.add_workspace_tag(ids[0], "parked").unwrap();
    // A tag used only in another repo is still offered.
    let other = app.store.add_repo(std::path::Path::new("/tmp/o"), "other", "").unwrap();
    let ow = app.store.insert_workspace(&crate::data::store::NewWorkspace {
        repo_id: other, name: "o", branch: "o", worktree_path: std::path::Path::new("/tmp/wsx-o"),
        yolo: false, agent: crate::pty::session::AgentKind::Claude, shared: false,
    }).unwrap();
    app.store.add_workspace_tag(ow, "ui").unwrap();
    app.refresh().unwrap();
    open_picker(&mut app).await;
    let Some(crate::ui::modal::Modal::HideRules { rows, .. }) = &app.modal else { panic!() };
    assert_eq!(rows[0], (HideRule::tag("parked"), 1));
    assert_eq!(rows[1], (HideRule::tag("ui"), 0), "counts are this repo only");
    assert_eq!(rows[2].0, MERGED);
}

#[tokio::test]
async fn stale_tag_rule_is_listed_with_zero_count() {
    let (mut app, repo, _) = app_with_merged_hidden();
    app.store.set_repo_hide_rules(repo, &HideRules::parse(Some("tag:gone,pr:merged"))).unwrap();
    app.refresh().unwrap();
    open_picker(&mut app).await;
    let Some(crate::ui::modal::Modal::HideRules { rows, rules, .. }) = &app.modal else { panic!() };
    assert_eq!(rows[0], (HideRule::tag("gone"), 0));
    assert!(rules.contains(&HideRule::tag("gone")));
}

#[tokio::test]
async fn space_on_tag_row_toggles_tag_rule() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    app.store.add_workspace_tag(ids[1], "parked").unwrap();
    app.refresh().unwrap();
    open_picker(&mut app).await;
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Char(' '))).await.unwrap();
    let stored = app.store.repos().unwrap().into_iter().find(|r| r.id == repo).unwrap();
    assert!(stored.hide_rules.contains(&HideRule::tag("parked")));
}
```

Render test in `src/ui/modal/mod.rs` tests:

```rust
    #[test]
    fn hide_rules_picker_draws_section_headers() {
        use crate::data::hide_rules::{HideRule, HideRules, PrCriterion};
        let m = Modal::HideRules {
            repo_id: crate::data::store::RepoId(1),
            repo_name: "wsx".into(),
            rules: HideRules::parse(Some("tag:parked")),
            rows: vec![(HideRule::tag("parked"), 2), (HideRule::Pr(PrCriterion::Merged), 0)],
            selected: 0,
        };
        let text = render_to_text(&m);
        let tagged = text.find("tagged").expect("tag header");
        let pr = text.find("PR state").expect("pr header");
        assert!(tagged < text.find("#parked").unwrap() && text.find("#parked").unwrap() < pr);
        assert!(text.contains("[x] #parked"));
    }
```

- [ ] **Step 2: Run** `cargo test hide_rules` — FAIL.

- [ ] **Step 3: Implement.**
  - Variant: replace `counts: [usize; 6]` with `/// Tag rows (sorted) then PR rows, with this repo's match counts at open.` `rows: Vec<(crate::data::hide_rules::HideRule, usize)>,`.
  - Row builder in `pickers.rs`:

```rust
/// Every tag in use plus any this repo still has a rule for, then PR rules.
pub(crate) fn hide_rule_rows(
    app: &App,
    repo: crate::data::store::RepoId,
    rules: &crate::data::hide_rules::HideRules,
) -> Result<Vec<(crate::data::hide_rules::HideRule, usize)>> {
    use crate::data::hide_rules::HideRule;
    let mut tag_rules: Vec<HideRule> = app.store.all_tags()?.iter().map(|(t, _)| HideRule::tag(t)).collect();
    tag_rules.extend(rules.iter().filter(|r| matches!(r, HideRule::Tag(_))).cloned());
    tag_rules.sort();
    tag_rules.dedup();
    let ids: Vec<_> = app.workspaces.iter().filter(|(rid, _)| *rid == repo).map(|(_, w)| w.id).collect();
    Ok(tag_rules
        .into_iter()
        .chain(HideRule::PR_RULES)
        .map(|rule| {
            let n = ids.iter().filter(|id| rule.matches(&app.hide_subject(**id))).count();
            (rule, n)
        })
        .collect())
}
```

  - `F` handler in `src/app/input/dashboard.rs`: replace the `counts` block with `let rows = crate::app::input::modal::pickers::hide_rule_rows(app, repo_id, &repo.hide_rules)?;` (clone `repo.hide_rules`/`repo.name` first to end the borrow on `app.repos`).
  - `hide_rules_picker`: take `rows` instead of `counts`; `last = rows.len().saturating_sub(1)`; `let rule = rows[selected].0.clone();` then `rules.toggle(rule.clone())`, `rules.contains(&rule)`, `expanded_hidden.remove(&(repo_id, rule))`; reopen with `rows`.
  - Render arm:

```rust
        Modal::HideRules { repo_name, rules, rows, selected, .. } => {
            use crate::data::hide_rules::HideRule;
            let mut lines = Vec::new();
            let mut last_tag: Option<bool> = None;
            for (i, (rule, n)) in rows.iter().enumerate() {
                let is_tag = matches!(rule, HideRule::Tag(_));
                if last_tag != Some(is_tag) {
                    lines.push(if is_tag { "  tagged" } else { "  PR state" }.to_string());
                    last_tag = Some(is_tag);
                }
                let cursor = if i == *selected { ">" } else { " " };
                let mark = if rules.contains(rule) { "x" } else { " " };
                lines.push(format!("{cursor} [{mark}] {:<14} {n:>3}", rule.label()));
            }
            (
                "hide workspaces",
                format!("Hide {repo_name} workspaces that are:\n\n{}\n\n\
                         \u{2191}\u{2193} move   space toggle   Esc close", lines.join("\n")),
            )
        }
```

  - Dispatch in `src/app/input/modal/mod.rs`: `rows` instead of `counts`.
  - Long tag labels: `{:<14}` pads short ones; longer labels just push the count right, acceptable.

- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings` — green.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(tui): tag rules in the F hide-rules picker"
```

---

### Task 7: Docs

**Files:**
- Create: `docs/book/src/daily-use/workspace-tags.md`
- Modify: `docs/book/src/SUMMARY.md:19`, `docs/book/src/daily-use/hiding-workspaces.md`, `docs/book/src/daily-use/keybindings.md:17,44-52`, `docs/book/src/cli-reference/workspace-management.md`, spec `Status:` → `implemented`

- [ ] **Step 1: Write `workspace-tags.md`:**

```markdown
# Workspace tags

Tags are short labels you put on workspaces, like `parked` or `spike`. They
show on the dashboard row, narrow the `/` filter, and can
[hide workspaces](hiding-workspaces.md#by-tag).

## Adding and removing tags

Press `#` on a workspace (or `#` on its `?` card). Type a tag and press
`enter` to add it, or move to an existing one and press `space` to toggle
it. The list offers every tag used anywhere in wsx.

Tags are lowercased and may use `a-z`, `0-9`, `-` and `_`, up to 32
characters.

From a shell or an agent:

    wsx workspace tag add parked
    wsx workspace tag rm parked --workspace backend/old-spike
    wsx workspace tag list --all

`add` and `rm` act on the current workspace unless `--workspace
<repo>/<slug>` is given, and print its tags afterwards. `list --all` prints
every tag in use with how many workspaces carry it. `wsx workspace list
--json` includes each workspace's `tags`.

## On the dashboard

Tags appear dimmed after the workspace name: `my-slug #parked #ui`. On a
narrow row they give way to the name first, leaving `+2` for the tags that
didn't fit.

## Searching

`/parked` matches tags along with names and recaps. `/#parked` matches only
workspaces tagged exactly `parked`.

Tags are unrelated to [prompt tags](prompt-tags.md).
```

- [ ] **Step 2: `hiding-workspaces.md`:** change the intro to "by PR state or by tag"; after the PR-state section add:

```markdown
## By tag

The `F` picker also lists every [workspace tag](workspace-tags.md) in use,
above the PR states. Turning one on hides that repo's workspaces carrying
the tag, whatever they're doing:

    ▸ #parked  ·  2 hidden

A workspace matching both a tag rule and a PR rule is grouped under the
tag. A rule for a tag no workspace carries any more stays in the picker,
with a count of 0, so you can turn it off.
```

- [ ] **Step 3: `keybindings.md`:** dashboard table row `| \`#\` | Edit the selected workspace's [tags](workspace-tags.md) |`; change the `Shift + F` text to "Choose which PR states and tags hide workspaces in the selected row's repo …"; actions-card table row `| \`#\` | Edit the workspace's tags |`.

- [ ] **Step 4: `SUMMARY.md`:** add `  - [Workspace tags](daily-use/workspace-tags.md)` right after the Hiding workspaces line.

- [ ] **Step 5: `cli-reference/workspace-management.md`:** append a section in the file's existing style:

````markdown
```
wsx workspace tag add|rm <tag>... [--workspace <repo>/<slug>]
wsx workspace tag list [--workspace <repo>/<slug> | --all] [--json]
```

Adds, removes, or lists [workspace tags](../daily-use/workspace-tags.md). Targets the current workspace unless `--workspace` is given. If any tag in an `add` or `rm` is invalid, nothing is written and the command fails naming it. `list --all` prints `tag<TAB>count` for every tag in use.
````

- [ ] **Step 6:** set spec `**Status:** implemented`. If `mdbook` is installed, run `mdbook build docs/book` and check for broken links; otherwise skip.

- [ ] **Step 7: Commit**

```bash
git add docs
git commit -m "docs: workspace tags"
```

---

### Task 8: Final verification

- [ ] `git fetch origin && git rebase origin/main` (resolve conflicts if any).
- [ ] `cargo test` — all pass.
- [ ] `cargo clippy --all-targets -- -D warnings` — clean.
- [ ] `cargo fmt --check` — clean (run `cargo fmt` and amend the relevant commit if not).
- [ ] Manual: `cargo build --release`, launch `target/release/wsx` (do NOT replace `~/.local/bin/wsx`), tag a workspace with `#`, toggle a tag rule with `F`, confirm the `# tag · N hidden` row, `/#tag` filter, and `wsx workspace tag list --all`.
- [ ] Report "committed locally, ready to push/open PR when you say so".
