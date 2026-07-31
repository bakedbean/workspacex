# macOS menubar Project Manager submenu — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Project Manager` submenu to the macOS SwiftBar menubar dropdown that shows each workspace's agent-authored recap (goal / state / next), ordered blocked → waiting → stalest-first.

**Architecture:** The SwiftBar plugin (`wsx menubar plugin`) is a short-lived process that renders the whole dropdown from SQLite with zero subprocesses. This feature keeps that contract: it adds one query (`all_workspace_recaps`) and appends a new section of menu lines. Rendering logic goes in a new `src/menubar/pm.rs`; the SwiftBar escaping helpers move out of `plugin.rs` into `src/menubar/escape.rs` so both modules can share them.

**Tech Stack:** Rust 2024 (pinned in `rust-toolchain.toml`), `rusqlite`, SwiftBar plugin protocol. Tests are inline `#[cfg(test)] mod` blocks, run with `cargo test`.

## Global Constraints

- **Read the spec first:** `docs/superpowers/specs/2026-07-31-menubar-pm-submenu-design.md`. It is the authority; this plan implements it.
- **Cache-only render path.** No git, `gh`, filesystem, or subprocess work may be added to `wsx menubar plugin`. Only SQLite reads.
- **`src/menubar/` is macOS-only** (`#[cfg(target_os = "macos")] pub mod menubar;` in `src/lib.rs:12-13`). Core never depends on it. `src/waybar/` is Linux-only and **cannot be compiled on this machine** — if a change touches shared code, CI is the gate.
- **Time units:** `workspace_recap.updated_at` and `workspace_status.reported_at` are epoch **milliseconds**; `scm_cache.fetched_at` / `git_fetched_at` are epoch **seconds**. This feature touches only the first two — always use `crate::time::now_ms()`, never `workspace_rows::unix_now()`.
- **Escaping is mandatory** for every agent- or user-authored string that reaches a menu line. Recap text is agent-authored.
- **Commit messages:** conventional commits. Do **not** add `Co-Authored-By` or "Generated with Claude Code" trailers.
- **Verify before claiming done:** run `cargo test --all-targets --all-features` and `cargo clippy --all-targets --all-features -- -D warnings` and paste real output. `RUSTFLAGS: -D warnings` is set in CI, so warnings fail the build.

## File Structure

| File | Responsibility |
|---|---|
| `src/time.rs` | Wall-clock helpers. Gains `format_age` (moved from the TUI). |
| `src/ui/updates_bar.rs` | TUI updates bar. Loses `format_age`, re-exports it. |
| `src/menubar/escape.rs` | **New.** SwiftBar string escaping: injection barrier, length caps, param quoting. |
| `src/menubar/pm.rs` | **New.** The PM section: card model, ordering, line rendering. |
| `src/menubar/plugin.rs` | SwiftBar document assembly. Loses escaping, gains the PM section call. |
| `src/menubar/mod.rs` | Module declarations. |
| `src/workspace_rows.rs` | Shared Linux/macOS row collection. `RowInput` gains `id`. |
| `docs/manual-tests/menubar.md` | Manual verification procedure. |
| `README.md` | User-facing menubar docs. |

---

### Task 1: Move `format_age` to `time.rs`

`format_age` is a pure duration formatter that currently lives in a ratatui widget module. `src/menubar/pm.rs` needs it, and the menubar module must not depend on TUI rendering code.

**Files:**
- Modify: `src/time.rs` (append function + tests)
- Modify: `src/ui/updates_bar.rs:305-314` (delete function), `:549-554` (delete its tests)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `crate::time::format_age(delta_ms: i64) -> String`. Also still reachable as `crate::ui::updates_bar::format_age` via re-export.

- [ ] **Step 1: Write the failing test**

Append to `src/time.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_age_buckets_by_magnitude() {
        assert_eq!(format_age(0), "0s");
        assert_eq!(format_age(59_999), "59s");
        assert_eq!(format_age(60_000), "1m");
        assert_eq!(format_age(3_599_000), "59m");
        assert_eq!(format_age(3_600_000), "1h");
        assert_eq!(format_age(-500), "0s"); // negative delta clamps
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib time::tests::format_age_buckets_by_magnitude`
Expected: FAIL — compile error, `cannot find function 'format_age' in this scope`.

- [ ] **Step 3: Move the function**

Add to `src/time.rs`, above the `#[cfg(test)]` block:

```rust
/// Human-readable age from a millisecond delta: `45s`, `12m`, `3h`.
/// Negative deltas (clock skew) clamp to `0s`.
pub fn format_age(delta_ms: i64) -> String {
    let secs = (delta_ms / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}
```

In `src/ui/updates_bar.rs`, delete the `pub fn format_age` definition (lines 305-314) and put this in its place:

```rust
// Moved to `crate::time` so non-TUI callers (the macOS menubar) can use it
// without depending on ratatui widget code. Re-exported here because the
// updates bar, the updates panel, and the PM digest all reach it by this path.
pub use crate::time::format_age;
```

Then delete the now-duplicated assertions from the `format_age` test in `src/ui/updates_bar.rs` (lines 549-554). If those assertions are the entire body of their `#[test]` fn, delete the whole fn.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib time:: && cargo test --lib updates_bar`
Expected: PASS. The re-export means `ui/pm_pane.rs`, `ui/modal/updates_panel.rs`, and `ui/updates_bar.rs` call sites compile untouched.

- [ ] **Step 5: Check for warnings**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean. (A `pub use` that nothing in the module itself uses is fine — it is a public re-export, not an unused import.)

- [ ] **Step 6: Commit**

```bash
git add src/time.rs src/ui/updates_bar.rs
git commit -m "refactor: move format_age to time.rs

The macOS menubar needs a duration formatter and must not depend on
ratatui widget code. updates_bar re-exports it so call sites are
unchanged."
```

---

### Task 2: Extract SwiftBar escaping into `escape.rs`

`plugin.rs` owns the escaping helpers today. `pm.rs` needs them, and `plugin.rs` should not become a utility module. This task is a pure move plus one new primitive; no behavior changes.

**Files:**
- Create: `src/menubar/escape.rs`
- Modify: `src/menubar/mod.rs`, `src/menubar/plugin.rs:33-68` (delete moved items), `:13` (imports), `:418-423` and `:466-481` (move/split tests)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `pub(crate) const MAX_TEXT_LEN: usize = 120;`
  - `pub(crate) fn esc_core(s: &str) -> String`
  - `pub(crate) fn esc_text_uncapped(s: &str) -> String`
  - `pub(crate) fn esc_text(s: &str) -> String`
  - `pub(crate) fn quote_param(s: &str) -> String`

- [ ] **Step 1: Create the module with its tests**

Create `src/menubar/escape.rs`:

```rust
//! SwiftBar string escaping: the injection barrier between wsx data and the
//! menu protocol, plus length caps and param quoting. Shared by the plugin
//! document renderer and the Project Manager section.

use crate::workspace_rows::sanitize;

/// Cap on a rendered line's *display text* segment, in chars — keeps one
/// hostile/huge status message from ballooning the SwiftBar document. Never
/// applied to param values (paths, URLs), which must survive intact or the
/// action they drive (open path, open URL) breaks.
pub(crate) const MAX_TEXT_LEN: usize = 120;

/// Injection barrier shared by display text and param values: control chars
/// collapse (via sanitize) and the protocol's text/params separator '|'
/// becomes a broken bar, so no user-controlled string can smuggle params or
/// extra rows. Uncapped and no dash guard — safe for param values (quoted,
/// not line-initial) where truncation would corrupt a real path or URL.
pub(crate) fn esc_core(s: &str) -> String {
    sanitize(s).replace('|', "\u{00a6}")
}

/// `esc_core` plus a guard on a leading '-' (so the string can't read as a
/// '---' separator or '--' submenu marker), with no length cap. The shared
/// primitive: `esc_text` caps it plainly, while the PM section caps it with
/// an ellipsis. A single capped helper cannot serve both.
pub(crate) fn esc_text_uncapped(s: &str) -> String {
    let mut out = esc_core(s);
    if out.starts_with('-') {
        out.replace_range(0..1, "\u{2011}");
    }
    out
}

/// Display-text sanitizer: `esc_text_uncapped` truncated to `MAX_TEXT_LEN`.
/// Only for text that renders directly on a menu line — never for param
/// values.
pub(crate) fn esc_text(s: &str) -> String {
    let out = esc_text_uncapped(s);
    if out.chars().count() > MAX_TEXT_LEN {
        return out.chars().take(MAX_TEXT_LEN).collect();
    }
    out
}

/// All bash=/paramN=/href= values are double-quoted; interior quotes degrade
/// to '\'' (a path with a double quote is pathological — keeping the
/// protocol unbreakable beats preserving it). Uses the uncapped, unguarded
/// `esc_core` — param values are real paths/URLs consumed by the action they
/// drive, not display text, so they must never be truncated or dash-shifted.
pub(crate) fn quote_param(s: &str) -> String {
    format!("\"{}\"", esc_core(s).replace('"', "'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_text_caps_length() {
        let huge = "x".repeat(1000);
        let capped = esc_text(&huge);
        assert_eq!(capped.chars().count(), MAX_TEXT_LEN);
        assert_eq!(capped, "x".repeat(MAX_TEXT_LEN));
    }

    #[test]
    fn esc_text_uncapped_does_not_truncate() {
        let huge = "x".repeat(1000);
        assert_eq!(esc_text_uncapped(&huge).chars().count(), 1000);
    }

    #[test]
    fn esc_text_guards_leading_dash() {
        // A repo named "---evil" must not render a line that IS a bare
        // separator once escaped.
        let escaped = esc_text("---evil");
        assert_ne!(escaped, "---");
        assert!(!escaped.starts_with('-'), "{escaped}");
    }

    #[test]
    fn esc_core_neutralizes_pipes_and_control_chars() {
        assert_eq!(esc_core("a|b"), "a\u{00a6}b");
        assert_eq!(esc_core("a\nb\tc"), "a b c");
        // No dash guard, no cap — param values must survive intact.
        assert_eq!(esc_core("-x"), "-x");
    }

    #[test]
    fn quote_param_wraps_and_degrades_double_quotes() {
        assert_eq!(quote_param("/a/b"), "\"/a/b\"");
        assert_eq!(quote_param("/a/\"b\""), "\"/a/'b'\"");
    }
}
```

- [ ] **Step 2: Declare the module**

In `src/menubar/mod.rs`, add `pub mod escape;` keeping the list alphabetical:

```rust
pub mod escape;
pub mod install;
pub mod jump;
pub mod plugin;
pub mod refresh;
```

- [ ] **Step 3: Run the new tests to verify they pass**

Run: `cargo test --lib menubar::escape`
Expected: PASS (5 tests). The module compiles standalone; `plugin.rs` still has its own copies at this point, which is why nothing is broken yet.

- [ ] **Step 4: Delete the duplicates from `plugin.rs`**

In `src/menubar/plugin.rs`:

1. Delete `MAX_TEXT_LEN` (line 33), `esc_core` (41-43), `esc_text` (49-58), and `quote_param` (66-68), along with their doc comments.
2. Replace the `use` on line 13 with:

```rust
use crate::menubar::escape::{esc_text, quote_param};
use crate::workspace_rows::{RowInput, attention_rank, collect_rows_cached, state_glyph};
```

   (`sanitize` was only used by `esc_core`, which has moved; `esc_core` itself is not used directly by `plugin.rs`.)
3. In the `plugin_tests` module, delete `esc_text_caps_length` (418-423) and, from `esc_text_guards_leading_dash` (466-481), delete only the first three lines of assertions (the `esc_text("---evil")` block) — both now live in `escape.rs`. Keep the rest of that test, which exercises `submenu_lines`, and rename it:

```rust
    #[test]
    fn leading_dash_status_stays_inside_its_subtitle_line() {
        // A status message starting with "-- " must stay embedded inside
        // its single subtitle line, not spawn an extra menu row.
        let mut r = row("r", "w");
        r.status = Some(status(ReportedState::Working, Some("-- fake")));
        let lines = submenu_lines(&r, "/bin/wsx");
        assert!(lines[0].starts_with("-- "), "{:?}", lines[0]);
        assert!(lines[0].contains("-- fake"), "{:?}", lines[0]);
        assert_eq!(lines[0].matches("\n").count(), 0);
    }
```

- [ ] **Step 5: Run the full suite**

Run: `cargo test --lib menubar && cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS, no warnings. Every pre-existing `plugin_tests` test still passes — this task changed no behavior.

- [ ] **Step 6: Commit**

```bash
git add src/menubar/escape.rs src/menubar/mod.rs src/menubar/plugin.rs
git commit -m "refactor(menubar): extract SwiftBar escaping into escape.rs

The Project Manager section needs the same injection barrier, and
plugin.rs should stay a document renderer rather than become a utility
module. Adds esc_text_uncapped as the shared primitive: esc_text caps it
plainly, and the PM section will cap it with an ellipsis."
```

---

### Task 3: Carry the workspace id on `RowInput`

`collect_rows_cached` builds rows from `WsMeta`, which has the workspace id, but drops it. Without it there is nothing to join recaps against.

**Files:**
- Modify: `src/workspace_rows.rs:61-68` (struct), `:121-128` and `:168-175` (constructors), `:286-295` (test)
- Modify: `src/menubar/plugin.rs:241-250` (test helper)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `RowInput.id: crate::data::store::WorkspaceId` (a `Copy` newtype over `i64`).

- [ ] **Step 1: Write the failing test**

In `src/workspace_rows.rs`, extend the existing `cached_collect_reads_cache_without_git` test. After the `let rows = collect_rows_cached(&store).unwrap();` line and the `assert_eq!(rows.len(), 1);`, add:

```rust
        // The id is what lets callers join per-workspace tables (recaps,
        // status) onto a row.
        assert_eq!(rows[0].id, id);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib workspace_rows::tests::cached_collect_reads_cache_without_git`
Expected: FAIL — compile error, `no field 'id' on type 'RowInput'`.

- [ ] **Step 3: Add and populate the field**

In `src/workspace_rows.rs`, add `id` as the first field of `RowInput`:

```rust
pub struct RowInput {
    pub id: crate::data::store::WorkspaceId,
    pub repo_name: String,
    pub slug: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub status: Option<ReportedStatus>,
    pub cache: ScmCacheRow,
}
```

In `collect_rows_cached`, add `id: m.id,` to the `RowInput { ... }` literal (`WorkspaceId` is `Copy`, so this is fine even though `m`'s other fields are moved).

In `collect_rows_fresh`, add `id: m.id,` to the `rows.push(RowInput { ... })` literal.

In `src/menubar/plugin.rs`, add the field to the `row` test helper:

```rust
    fn row(repo: &str, slug: &str) -> RowInput {
        RowInput {
            id: crate::data::store::WorkspaceId(0),
            repo_name: repo.into(),
            slug: slug.into(),
            branch: format!("x/{slug}"),
            worktree_path: format!("/wt/{repo}/{slug}").into(),
            status: None,
            cache: ScmCacheRow::default(),
        }
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib workspace_rows && cargo test --lib menubar`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workspace_rows.rs src/menubar/plugin.rs
git commit -m "refactor: carry the workspace id on RowInput

collect_rows_cached had the id in WsMeta and discarded it, leaving
callers nothing to join per-workspace tables against."
```

**Note for the reviewer:** `src/waybar/` also consumes `RowInput` but never constructs one (it only reads fields from `collect_rows_fresh`), so this is additive there. Linux cannot be compiled on a macOS dev machine — CI's `ubuntu-latest` test job is the gate.

---

### Task 4: The PM section (`pm.rs`)

All of `pm.rs`: the card model, the ordering, and the line rendering. Built in two TDD cycles — data first, then rendering — but one task and one commit, because the two halves cannot compile-verify independently (the card model is unused, and warns as dead code, until the renderer calls it).

**Files:**
- Create: `src/menubar/pm.rs`
- Modify: `src/menubar/mod.rs`, `src/menubar/plugin.rs:16` (widen `ROW_FONT`)

**Interfaces:**
- Consumes: `RowInput.id` (Task 3); `esc_text`, `esc_text_uncapped`, `quote_param` (Task 2); `crate::time::format_age` (Task 1).
- Produces:
  - `pub(crate) struct PmCard<'a> { pub row: &'a RowInput, pub recap: Option<&'a WorkspaceRecap> }`
  - `pub(crate) fn cards_for_repo<'a>(rows: &'a [RowInput], recaps: &'a HashMap<WorkspaceId, WorkspaceRecap>, repo: &str) -> Vec<PmCard<'a>>`
  - `pub(crate) fn pm_section_lines(repo_names: &[String], rows: &[RowInput], recaps: &HashMap<WorkspaceId, WorkspaceRecap>, wsx_bin: &str, now_ms: i64) -> Vec<String>`
  - `pub(crate) const MAX_RECAP_LEN: usize`, `pub(crate) const RECAP_INDENT: &str`

#### Cycle A — card model and ordering

- [ ] **Step 1: Write the failing tests**

Create `src/menubar/pm.rs`:

```rust
//! The SwiftBar `Project Manager` section: each workspace's agent-authored
//! recap (goal / state / next), ordered blocked → waiting → stalest-first.
//! See docs/superpowers/specs/2026-07-31-menubar-pm-submenu-design.md.

use std::collections::HashMap;

use crate::data::store::{ReportedStatus, WorkspaceId, WorkspaceRecap};
use crate::workspace_rows::RowInput;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::scm_cache::ScmCacheRow;
    use crate::data::store::ReportedState;

    fn row(repo: &str, slug: &str, id: i64) -> RowInput {
        RowInput {
            id: WorkspaceId(id),
            repo_name: repo.into(),
            slug: slug.into(),
            branch: format!("x/{slug}"),
            worktree_path: format!("/wt/{repo}/{slug}").into(),
            status: None,
            cache: ScmCacheRow::default(),
        }
    }

    fn status(state: ReportedState, at: i64) -> ReportedStatus {
        ReportedStatus {
            state,
            message: Some("msg".into()),
            source: "test".into(),
            reported_at: at,
        }
    }

    fn recap(goal: Option<&str>, at: i64) -> WorkspaceRecap {
        WorkspaceRecap {
            goal: goal.map(str::to_string),
            state: Some("s".into()),
            next: Some("n".into()),
            updated_at: at,
        }
    }

    #[test]
    fn orders_blocked_then_waiting_then_stalest_first() {
        let rows = vec![
            row("alpha", "fresh-working", 1),
            row("alpha", "stale-working", 2),
            row("alpha", "waiting", 3),
            row("alpha", "blocked", 4),
        ];
        let mut rows = rows;
        rows[0].status = Some(status(ReportedState::Working, 9_000));
        rows[1].status = Some(status(ReportedState::Working, 1_000));
        rows[2].status = Some(status(ReportedState::Waiting, 5_000));
        rows[3].status = Some(status(ReportedState::Blocked, 5_000));
        let cards = cards_for_repo(&rows, &HashMap::new(), "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["blocked", "waiting", "stale-working", "fresh-working"]);
    }

    #[test]
    fn recap_updated_at_counts_toward_the_stalest_tiebreak() {
        // Both working; the one whose most recent agent signal is older
        // sorts first, even when that signal is the recap, not the status.
        let mut rows = vec![row("alpha", "recent-recap", 1), row("alpha", "old-recap", 2)];
        rows[0].status = Some(status(ReportedState::Working, 1_000));
        rows[1].status = Some(status(ReportedState::Working, 1_000));
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("g"), 9_000));
        recaps.insert(WorkspaceId(2), recap(Some("g"), 2_000));
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["old-recap", "recent-recap"]);
    }

    #[test]
    fn never_seen_workspace_sorts_to_the_top_of_its_rank() {
        let mut rows = vec![row("alpha", "seen", 1), row("alpha", "never", 2)];
        rows[0].status = Some(status(ReportedState::Working, 1_000));
        // "never" has no status and no recap at all → signal 0.
        let cards = cards_for_repo(&rows, &HashMap::new(), "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["never", "seen"]);
    }

    #[test]
    fn done_and_working_share_the_lowest_rank() {
        // Parity with the TUI digest: only blocked and waiting are ranked
        // ahead. (workspace_rows::attention_rank, used for the header
        // color, ranks Done above Waiting — deliberately not reused here.)
        let mut rows = vec![row("alpha", "done", 1), row("alpha", "waiting", 2)];
        rows[0].status = Some(status(ReportedState::Done, 5_000));
        rows[1].status = Some(status(ReportedState::Waiting, 9_000));
        let cards = cards_for_repo(&rows, &HashMap::new(), "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["waiting", "done"]);
    }

    #[test]
    fn filters_to_the_named_repo() {
        let rows = vec![row("alpha", "a1", 1), row("beta", "b1", 2)];
        let cards = cards_for_repo(&rows, &HashMap::new(), "beta");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].row.slug, "b1");
    }

    #[test]
    fn all_empty_recap_row_is_treated_as_absent() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: None,
                state: Some(String::new()), // present but empty
                next: None,
                updated_at: 9_000,
            },
        );
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        assert!(cards[0].recap.is_none(), "empty fields → no recap");
    }

    #[test]
    fn partially_filled_recap_is_kept() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("only a goal".into()),
                state: None,
                next: None,
                updated_at: 9_000,
            },
        );
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        assert_eq!(cards[0].recap.unwrap().goal.as_deref(), Some("only a goal"));
    }

    #[test]
    fn absent_recap_contributes_no_signal() {
        // The all-empty recap's updated_at (9_000) must not make this
        // workspace look freshly touched.
        let mut rows = vec![row("alpha", "empty-recap", 1), row("alpha", "real", 2)];
        rows[1].status = Some(status(ReportedState::Working, 5_000));
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap { goal: None, state: None, next: None, updated_at: 9_000 },
        );
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["empty-recap", "real"]);
    }
}
```

- [ ] **Step 2: Declare the module and run the tests to verify they fail**

In `src/menubar/mod.rs` add `pub mod pm;` after `pub mod jump;`.

Run: `cargo test --lib menubar::pm`
Expected: FAIL — compile errors, `cannot find function 'cards_for_repo'`, `cannot find type 'PmCard'`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` block in `src/menubar/pm.rs`:

```rust
/// One workspace's PM entry: its menu row plus the recap narrative, if any.
pub(crate) struct PmCard<'a> {
    pub row: &'a RowInput,
    /// `None` when there is no recap row, or when the row exists but all
    /// three fields are absent/empty — the two cases render identically and
    /// must be indistinguishable to every consumer.
    pub recap: Option<&'a WorkspaceRecap>,
}

/// Needs-attention rank: blocked (0) before waiting (1) before the rest (2).
///
/// Mirrors the TUI digest's `ui::pm_pane::attention_rank`. Deliberately NOT
/// `workspace_rows::attention_rank`, which ranks descending and puts `Done`
/// above `Waiting` — that one exists to pick the menubar header's worst
/// state, a different question.
fn pm_attention_rank(status: Option<&ReportedStatus>) -> u8 {
    use crate::data::store::ReportedState;
    match status.map(|s| s.state) {
        Some(ReportedState::Blocked) => 0,
        Some(ReportedState::Waiting) => 1,
        _ => 2,
    }
}

/// A recap counts only if at least one field carries text. A row whose
/// fields are all NULL or empty is reachable through the CLI's partial
/// upsert and must read as "no recap yet".
fn effective_recap(recap: Option<&WorkspaceRecap>) -> Option<&WorkspaceRecap> {
    let r = recap?;
    let has_text = [&r.goal, &r.state, &r.next]
        .into_iter()
        .any(|f| f.as_deref().is_some_and(|s| !s.trim().is_empty()));
    has_text.then_some(r)
}

/// Epoch ms of the last thing the agent said — a status push or a recap
/// update, whichever is newer; `0` when it has said nothing.
///
/// The DB-side stand-in for the TUI digest's session-log activity: the
/// plugin cannot tail JSONL, and `0` for "never" reproduces the TUI's own
/// `unwrap_or(0)`, floating never-seen workspaces to the top of their rank.
fn signal_ms(row: &RowInput, recap: Option<&WorkspaceRecap>) -> i64 {
    let from_status = row.status.as_ref().map(|s| s.reported_at).unwrap_or(0);
    let from_recap = recap.map(|r| r.updated_at).unwrap_or(0);
    from_status.max(from_recap)
}

/// This repo's cards, ordered blocked → waiting → rest, stalest first.
pub(crate) fn cards_for_repo<'a>(
    rows: &'a [RowInput],
    recaps: &'a HashMap<WorkspaceId, WorkspaceRecap>,
    repo: &str,
) -> Vec<PmCard<'a>> {
    let mut cards: Vec<PmCard<'a>> = rows
        .iter()
        .filter(|r| r.repo_name == repo)
        .map(|row| PmCard {
            row,
            recap: effective_recap(recaps.get(&row.id)),
        })
        .collect();
    cards.sort_by_key(|c| {
        (
            pm_attention_rank(c.row.status.as_ref()),
            signal_ms(c.row, c.recap),
        )
    });
    cards
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib menubar::pm`
Expected: PASS (8 tests).

At this point `cargo build` warns `dead_code` for `PmCard` and `cards_for_repo` — they are `pub(crate)` and only the test module calls them. That is expected mid-task; Cycle B adds the real caller. **Do not silence it** with `#[allow(dead_code)]`, and do not run the strict clippy gate until Step 9.

#### Cycle B — line rendering

- [ ] **Step 5: Write the failing rendering tests**

Add these to the `tests` module in `src/menubar/pm.rs`:

```rust
    fn section(rows: &[RowInput], recaps: &HashMap<WorkspaceId, WorkspaceRecap>) -> Vec<String> {
        pm_section_lines(
            &["alpha".into()],
            rows,
            recaps,
            "/bin/wsx",
            // 1h after the fixture timestamps below, so ages are stable.
            3_600_000,
        )
    }

    #[test]
    fn populated_card_renders_header_recap_and_facts() {
        use crate::git::forge::BranchLifecycle;
        let mut rows = vec![row("alpha", "api-fix", 1)];
        rows[0].status = Some(status(ReportedState::Blocked, 3_000_000));
        rows[0].cache = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrDraft),
            pr_number: Some(12),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            ..Default::default()
        };
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("add widgets endpoint"), 0));
        let lines = section(&rows, &recaps);
        let joined = lines.join("\n");

        // Header: glyph, slug, status word + age; the only clickable line.
        assert!(lines.iter().any(|l| l.starts_with("-- ! api-fix")), "{joined}");
        assert!(joined.contains("blocked 10m"), "{joined}");
        // The status *message* is not repeated here — it already appears in
        // the top-level row's own action submenu, and the recap lines are
        // the narrative this section exists for. ("msg" is the fixture's
        // status message.)
        assert!(!joined.contains("msg"), "{joined}");
        // Recap lines, each indented and disabled.
        assert!(
            lines.iter().any(|l| l.contains("goal:  add widgets endpoint")),
            "{joined}"
        );
        assert!(lines.iter().any(|l| l.contains("state: s")), "{joined}");
        assert!(lines.iter().any(|l| l.contains("next:  n")), "{joined}");
        // Facts: PR, dirty dot, diffstat, recap age — joined with " · ".
        let facts = lines.iter().find(|l| l.contains("#12 draft")).expect(&joined);
        assert!(facts.contains('\u{25cf}'), "{facts}");
        assert!(facts.contains("+45 -12"), "{facts}");
        assert!(facts.contains("recap 1h"), "{facts}");
        assert!(facts.contains(" \u{b7} "), "segments joined with a middot: {facts}");
    }

    #[test]
    fn only_the_header_line_is_clickable() {
        let mut rows = vec![row("alpha", "w", 1)];
        rows[0].status = Some(status(ReportedState::Working, 0));
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("g"), 0));
        let lines = section(&rows, &recaps);
        let clickable: Vec<&String> = lines.iter().filter(|l| l.contains("bash=")).collect();
        assert_eq!(clickable.len(), 1, "{lines:?}");
        assert!(clickable[0].contains("param2=\"jump\""), "{:?}", clickable[0]);
        assert!(clickable[0].contains("param3=\"alpha\""), "{:?}", clickable[0]);
        assert!(clickable[0].contains("param4=\"w\""), "{:?}", clickable[0]);
        // Everything below the header and the repo header is inert.
        for l in lines.iter().filter(|l| !l.contains("bash=")) {
            assert!(l.contains("disabled=true") || l == "-----", "{l}");
        }
    }

    #[test]
    fn card_without_recap_says_so_and_omits_recap_age() {
        let rows = vec![row("alpha", "w", 1)];
        let lines = section(&rows, &HashMap::new());
        let joined = lines.join("\n");
        assert!(joined.contains("no recap yet"), "{joined}");
        assert!(!joined.contains("recap 1h"), "{joined}");
        assert!(!joined.contains("goal:"), "{joined}");
    }

    #[test]
    fn card_with_no_facts_omits_the_facts_line() {
        // No status, no PR, not dirty, no recap: header + placeholder only.
        let rows = vec![row("alpha", "w", 1)];
        let lines = section(&rows, &HashMap::new());
        assert_eq!(lines.len(), 3, "repo header + card header + placeholder: {lines:?}");
    }

    #[test]
    fn partial_recap_renders_only_present_fields() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("only a goal".into()),
                state: None,
                next: None,
                updated_at: 0,
            },
        );
        let joined = section(&rows, &recaps).join("\n");
        assert!(joined.contains("goal:  only a goal"), "{joined}");
        assert!(!joined.contains("state:"), "{joined}");
        assert!(!joined.contains("next:"), "{joined}");
        assert!(!joined.contains("no recap yet"), "{joined}");
    }

    #[test]
    fn recap_lines_are_indented_with_the_indent_constant() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("g"), 0));
        let lines = section(&rows, &recaps);
        let goal = lines.iter().find(|l| l.contains("goal:")).unwrap();
        assert!(goal.starts_with(&format!("--{RECAP_INDENT}")), "{goal}");
        assert!(!goal.starts_with("-- "), "plain spaces would be trimmed: {goal}");
    }

    #[test]
    fn long_recap_field_is_capped_with_an_ellipsis() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some(&"g".repeat(200)), 0));
        let lines = section(&rows, &recaps);
        let goal = lines.iter().find(|l| l.contains("goal:")).unwrap();
        let text = goal.split(" | ").next().unwrap();
        // "--" + indent + "goal:  " + capped field.
        let field = text.rsplit("goal:  ").next().unwrap();
        assert_eq!(field.chars().count(), MAX_RECAP_LEN, "{field}");
        assert!(field.ends_with('\u{2026}'), "{field}");
    }

    #[test]
    fn short_recap_field_is_not_ellipsized() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("short"), 0));
        let joined = section(&rows, &recaps).join("\n");
        assert!(joined.contains("goal:  short"), "{joined}");
        assert!(!joined.contains('\u{2026}'), "{joined}");
    }

    #[test]
    fn recap_text_cannot_inject_lines_or_params() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("evil\n-- fake | bash=\"/bin/rm\"".into()),
                state: None,
                next: None,
                updated_at: 0,
            },
        );
        let lines = section(&rows, &recaps);
        for l in &lines {
            assert!(!l.contains('\n'), "{l}");
        }
        let goal = lines.iter().find(|l| l.contains("goal:")).unwrap();
        assert!(goal.contains('\u{00a6}'), "pipe not neutralized: {goal}");
        assert!(!goal.contains("bash=\"/bin/rm\""), "{goal}");
    }

    #[test]
    fn leading_dash_recap_cannot_become_a_separator() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("--- danger".into()),
                state: None,
                next: None,
                updated_at: 0,
            },
        );
        let joined = section(&rows, &recaps).join("\n");
        assert!(joined.contains('\u{2011}'), "leading dash not guarded: {joined}");
    }

    #[test]
    fn repos_and_cards_are_separated_without_a_trailing_separator() {
        let rows = vec![row("alpha", "a1", 1), row("alpha", "a2", 2), row("beta", "b1", 3)];
        let lines = pm_section_lines(
            &["alpha".into(), "beta".into()],
            &rows,
            &HashMap::new(),
            "/bin/wsx",
            0,
        );
        assert_eq!(lines[0], "-- alpha | disabled=true");
        assert!(lines.iter().any(|l| l == "-- beta | disabled=true"), "{lines:?}");
        assert!(lines.iter().any(|l| l == "-----"), "{lines:?}");
        assert_ne!(lines.last().unwrap(), "-----", "no trailing separator");
        assert_ne!(lines[0], "-----", "no leading separator");
    }

    #[test]
    fn empty_repo_renders_the_placeholder() {
        let lines = pm_section_lines(
            &["alpha".into()],
            &[],
            &HashMap::new(),
            "/bin/wsx",
            0,
        );
        assert_eq!(
            lines,
            vec![
                "-- alpha | disabled=true".to_string(),
                "-- (no workspaces) | disabled=true".to_string()
            ]
        );
    }
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test --lib menubar::pm`
Expected: FAIL — compile errors, `cannot find function 'pm_section_lines'`, `cannot find value 'RECAP_INDENT'`, `cannot find value 'MAX_RECAP_LEN'`.

- [ ] **Step 7: Write the implementation**

Add to the top of `src/menubar/pm.rs`, extending the existing `use` block:

```rust
use crate::menubar::escape::{esc_text, esc_text_uncapped, quote_param};
use crate::menubar::plugin::{ROW_FONT, pr_field};
use crate::time::format_age;
use crate::workspace_rows::state_glyph;
```

Then add, above the `#[cfg(test)]` block:

```rust
/// Cap on a rendered recap field, in chars, *including* the ellipsis.
/// Tighter than the document-wide `MAX_TEXT_LEN` because NSMenu sizes
/// itself to its widest item — one long goal line widens the whole
/// dropdown. Doctrine already asks agents for one-liners.
pub(crate) const MAX_RECAP_LEN: usize = 72;

/// Indent for a card's recap and fact lines.
///
/// Two NBSPs, because SwiftBar trims each line's title and ASCII spaces
/// would vanish. NBSP may not be enough: Swift's `CharacterSet.whitespaces`
/// includes U+00A0, and only a running SwiftBar can settle it. If the
/// manual test shows the lines flush with the header, change this to
/// `"\u{2502} "` (box-drawing light vertical + space) — not whitespace, so
/// it cannot be trimmed, and it reads as a continuation gutter. Tests
/// assert against this constant, so they hold under either value.
pub(crate) const RECAP_INDENT: &str = "\u{a0}\u{a0}";

/// A recap field escaped for display and capped at `MAX_RECAP_LEN`,
/// ellipsized when it overflows. Distinct from `esc_text`'s plain
/// truncation, which the top-level rows still use.
fn esc_recap(s: &str) -> String {
    let out = esc_text_uncapped(s);
    if out.chars().count() <= MAX_RECAP_LEN {
        return out;
    }
    let mut capped: String = out.chars().take(MAX_RECAP_LEN - 1).collect();
    capped.push('\u{2026}');
    capped
}

/// A card's lines: clickable header, one line per present recap field (or
/// the placeholder), and the facts line when any segment applies.
fn card_lines(card: &PmCard, wsx_bin: &str, now_ms: i64) -> Vec<String> {
    let r = card.row;
    let mut header = format!(
        "{} {}",
        state_glyph(r.status.as_ref().map(|s| s.state)),
        esc_text(&r.slug)
    );
    if let Some(s) = &r.status {
        header.push_str(&format!(
            "  {} {}",
            s.state.as_str(),
            format_age(now_ms - s.reported_at)
        ));
    }
    let mut out = vec![format!(
        "-- {header} | bash={} param1=\"menubar\" param2=\"jump\" param3={} param4={} terminal=false {ROW_FONT}",
        quote_param(wsx_bin),
        quote_param(&r.repo_name),
        quote_param(&r.slug),
    )];

    let detail = |text: String| format!("--{RECAP_INDENT}{text} | disabled=true {ROW_FONT}");
    match card.recap {
        Some(recap) => {
            for (label, field) in [
                ("goal:  ", &recap.goal),
                ("state: ", &recap.state),
                ("next:  ", &recap.next),
            ] {
                if let Some(v) = field.as_deref().filter(|v| !v.trim().is_empty()) {
                    out.push(detail(format!("{label}{}", esc_recap(v))));
                }
            }
        }
        None => out.push(detail("no recap yet".into())),
    }

    let mut segs: Vec<String> = Vec::new();
    let pr = pr_field(&r.cache);
    if !pr.is_empty() {
        segs.push(pr);
    }
    if r.cache.dirty == Some(true) {
        segs.push("\u{25cf}".into());
    }
    if let (Some(a), Some(d)) = (r.cache.additions, r.cache.deletions)
        && (a > 0 || d > 0)
    {
        segs.push(format!("+{a} -{d}"));
    }
    if let Some(recap) = card.recap {
        segs.push(format!("recap {}", format_age(now_ms - recap.updated_at)));
    }
    if !segs.is_empty() {
        out.push(detail(segs.join(" \u{b7} ")));
    }
    out
}

/// The whole `Project Manager` submenu body, at `--` depth: repos in the
/// document's order, each with its cards separated by `-----`, and no
/// leading or trailing separator.
pub(crate) fn pm_section_lines(
    repo_names: &[String],
    rows: &[RowInput],
    recaps: &HashMap<WorkspaceId, WorkspaceRecap>,
    wsx_bin: &str,
    now_ms: i64,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (i, repo) in repo_names.iter().enumerate() {
        if i > 0 {
            lines.push("-----".to_string());
        }
        lines.push(format!("-- {} | disabled=true", esc_text(repo)));
        let cards = cards_for_repo(rows, recaps, repo);
        if cards.is_empty() {
            lines.push("-- (no workspaces) | disabled=true".to_string());
            continue;
        }
        for (j, card) in cards.iter().enumerate() {
            if j > 0 {
                lines.push("-----".to_string());
            }
            lines.extend(card_lines(card, wsx_bin, now_ms));
        }
    }
    lines
}
```

Finally, `pm.rs` reads two items that are currently private to `plugin.rs`. In `src/menubar/plugin.rs`, widen them:

```rust
pub(crate) const ROW_FONT: &str = "font=SFMono-Regular size=12";
```

(`pr_field` is already `pub(crate)`; leave it.)

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --lib menubar::pm`
Expected: PASS (20 tests — 8 from Cycle A, 12 from Cycle B).

- [ ] **Step 9: Check formatting and lints**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean. The Cycle A dead-code warning is gone now that `pm_section_lines` calls `cards_for_repo`.

- [ ] **Step 10: Commit**

```bash
git add src/menubar/pm.rs src/menubar/mod.rs src/menubar/plugin.rs
git commit -m "feat(menubar): the Project Manager section renderer

Joins recaps onto menu rows, orders them blocked -> waiting ->
stalest-first using max(status.reported_at, recap.updated_at) as the
DB-side stand-in for the TUI's session-log tiebreak, and renders each
card as a clickable jump header plus its goal/state/next and fact
lines. Recap fields get a 72-char ellipsized cap because NSMenu sizes
itself to its widest item."
```

---

### Task 5: Wire the PM section into the document

**Files:**
- Modify: `src/menubar/plugin.rs:159-199` (render / document / plugin_document), `:382-415` (tests)

**Interfaces:**
- Consumes: `pm_section_lines` (Task 4).
- Produces: `render` and `document` gain `recaps: &HashMap<WorkspaceId, WorkspaceRecap>` and `now_ms: i64` parameters.

- [ ] **Step 1: Write the failing tests**

Add to `plugin_tests` in `src/menubar/plugin.rs`:

```rust
    #[test]
    fn pm_section_sits_between_the_repos_and_the_footer() {
        let rows = vec![row("alpha", "one")];
        let doc = render(
            &["alpha".into()],
            &rows,
            &std::collections::HashMap::new(),
            "/bin/wsx",
            0,
        );
        let lines: Vec<&str> = doc.lines().collect();
        let pm = lines.iter().position(|l| *l == "Project Manager").unwrap();
        let last_repo_row = lines.iter().rposition(|l| l.starts_with("\u{b7} one")).unwrap();
        let footer = lines.iter().position(|l| *l == "Refresh | refresh=true").unwrap();
        assert!(last_repo_row < pm && pm < footer, "{doc}");
        // Its own separator above, the footer's separator below.
        assert_eq!(lines[pm - 1], "---");
        assert_eq!(lines[footer - 1], "---");
        // The parent item is not disabled — a greyed submenu parent reads
        // as broken.
        assert!(!lines[pm].contains("disabled=true"), "{doc}");
        // And the body is present, at submenu depth.
        assert!(lines[pm + 1].starts_with("-- "), "{doc}");
    }

    #[test]
    fn pm_section_shows_recap_text_from_the_store_map() {
        let mut rows = vec![row("alpha", "one")];
        rows[0].id = crate::data::store::WorkspaceId(7);
        let mut recaps = std::collections::HashMap::new();
        recaps.insert(
            crate::data::store::WorkspaceId(7),
            crate::data::store::WorkspaceRecap {
                goal: Some("ship the thing".into()),
                state: None,
                next: None,
                updated_at: 0,
            },
        );
        let doc = render(&["alpha".into()], &rows, &recaps, "/bin/wsx", 0);
        assert!(doc.contains("goal:  ship the thing"), "{doc}");
    }

    #[test]
    fn empty_repo_list_still_has_no_pm_section() {
        let doc = document(&[], &[], &std::collections::HashMap::new(), "/bin/wsx", 0);
        assert_eq!(doc, error_header());
        assert!(!doc.contains("Project Manager"), "{doc}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib menubar::plugin`
Expected: FAIL — compile errors, `this function takes 3 arguments but 5 arguments were supplied`.

- [ ] **Step 3: Thread the new parameters through**

In `src/menubar/plugin.rs`, change `render`:

```rust
pub(crate) fn render(
    repo_names: &[String],
    rows: &[RowInput],
    recaps: &std::collections::HashMap<crate::data::store::WorkspaceId, crate::data::store::WorkspaceRecap>,
    wsx_bin: &str,
    now_ms: i64,
) -> String {
```

and, immediately before the existing footer push (`lines.push("---".into()); lines.push("Refresh | refresh=true".into());`), insert:

```rust
    lines.push("---".into());
    lines.push("Project Manager".into());
    lines.extend(crate::menubar::pm::pm_section_lines(
        repo_names, rows, recaps, wsx_bin, now_ms,
    ));
```

Change `document` to take and forward the same two parameters:

```rust
fn document(
    repo_names: &[String],
    rows: &[RowInput],
    recaps: &std::collections::HashMap<crate::data::store::WorkspaceId, crate::data::store::WorkspaceRecap>,
    wsx_bin: &str,
    now_ms: i64,
) -> String {
    if repo_names.is_empty() {
        return error_header();
    }
    render(repo_names, rows, recaps, wsx_bin, now_ms)
}
```

And `plugin_document`:

```rust
fn plugin_document(store: &Store, wsx_bin: &str) -> Result<String> {
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<String> = repos.into_iter().map(|r| r.name).collect();
    let rows = collect_rows_cached(store)?;
    let recaps = store.all_workspace_recaps()?;
    Ok(document(&names, &rows, &recaps, wsx_bin, crate::time::now_ms()))
}
```

- [ ] **Step 4: Update the three pre-existing call sites in tests**

In `plugin_tests`, `render_groups_by_repo_and_lists_empty_repos` and `empty_repo_list_is_icon_only_document` call the old signatures. Update them:

```rust
        let doc = render(
            &["alpha".into(), "beta".into(), "empty".into()],
            &rows,
            &std::collections::HashMap::new(),
            "/bin/wsx",
            0,
        );
```

```rust
        assert_eq!(
            document(&[], &[], &std::collections::HashMap::new(), "/bin/wsx", 0),
            error_header()
        );
```

`render_groups_by_repo_and_lists_empty_repos` also asserts `*lines.last().unwrap() == "Refresh | refresh=true"` — that still holds, because the PM section is inserted *above* the footer.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib menubar`
Expected: PASS. All pre-existing plugin tests still pass.

- [ ] **Step 6: Run the whole suite and lints**

Run: `cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features`
Expected: all clean/PASS. Paste the test summary line into your report.

- [ ] **Step 7: Verify the real document by hand**

Run: `cargo run -- menubar plugin`
Expected: the printed document ends with a `Project Manager` line, `--`-prefixed body lines showing your real workspaces and their recaps, then `---` and `Refresh | refresh=true`. This is the first end-to-end proof; read the output rather than trusting the tests.

- [ ] **Step 8: Commit**

```bash
git add src/menubar/plugin.rs
git commit -m "feat(menubar): Project Manager submenu in the SwiftBar document

Adds one SQLite query (all_workspace_recaps) to the render path and
appends the PM section between the workspace rows and the Refresh
footer. No subprocesses: the cache-only contract is unchanged."
```

---

### Task 6: Documentation

**Files:**
- Modify: `docs/manual-tests/menubar.md`, `README.md:67-85`

**Interfaces:**
- Consumes: the shipped feature.
- Produces: nothing code-facing.

- [ ] **Step 1: Add the manual test**

Append to the end of `docs/manual-tests/menubar.md`. The file currently ends at "Test 9: unusual repo/slug/message content", so this is Test 10 — no renumbering needed.

```markdown
## Test 10: Project Manager submenu

Prereq: at least one workspace with a recap. Set one with:

```
wsx recap set --goal "try the PM submenu" --state "checking rendering" --next "read the menu"
```

Click the menubar item and hover `Project Manager` (below the workspace
list, above `Refresh`).

Expected:

- The submenu opens and lists repos as headers, with each workspace's
  `goal:` / `state:` / `next:` lines beneath its name, and a facts line
  (`#12 draft · ● · +45 -12 · recap 2m`).
- Workspaces with no recap show `no recap yet`.
- Ordering differs from the list above: blocked first, then waiting, then
  the rest oldest-signal-first.
- Clicking a workspace's header line jumps to it, exactly like the
  top-level row.
- With many workspaces, the submenu scrolls rather than overflowing the
  screen.

Two rendering details cannot be unit-tested — check them explicitly:

1. **Separators.** The `-----` lines between workspaces must render as
   separator rules, not as literal `-----` text. If they render literally,
   change them in `pm_section_lines` to a `disabled=true` line containing a
   single NBSP.
2. **Indentation.** The recap and fact lines must sit indented under their
   workspace name. If they are flush left, SwiftBar trimmed the NBSPs —
   change `RECAP_INDENT` in `src/menubar/pm.rs` to `"\u{2502} "`.
```

- [ ] **Step 2: Update the README**

In the `## macOS menubar (SwiftBar)` section of `README.md`, add a paragraph after the existing description of what the dropdown contains:

```markdown
Below the workspace list, a **Project Manager** submenu shows each
workspace's agent-authored recap — the `goal` / `state` / `next` one-liners
maintained via `wsx recap set` — ordered blocked → waiting → least-recently
active. It is the menubar counterpart of the TUI's `p` view, rendered from
the same SQLite data, and clicking a workspace's line jumps to it.
```

- [ ] **Step 3: Verify the docs are accurate**

Run: `cargo run -- menubar plugin | head -40`
Read the output and confirm the README's claims match what actually renders. Fix the prose, not the output.

- [ ] **Step 4: Commit**

```bash
git add docs/manual-tests/menubar.md README.md
git commit -m "docs: menubar Project Manager submenu"
```

- [ ] **Step 5: Run the manual test**

Install the current build and exercise Test 4 for real:

```bash
cargo build --release && wsx setup menubar
```

**Note:** `~/.local/bin/wsx` is a symlink to `target/release/wsx` — never replace the symlink with a copy. `cargo build --release` is all that is needed to update it.

If either rendering detail fails, apply its documented fallback, re-run
`cargo test --lib menubar`, and commit the fix separately:

```bash
git commit -m "fix(menubar): <separator|indent> fallback per manual test"
```

Report the manual-test outcome — including which fallbacks (if any) were needed — rather than assuming it rendered correctly.

---

## Done criteria

- `cargo test --all-targets --all-features` passes; `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --all --check` are clean.
- `wsx menubar plugin` prints a `Project Manager` section between the workspace rows and the `Refresh` footer.
- The manual test in `docs/manual-tests/menubar.md` has been run against a live SwiftBar, and its two rendering unknowns are resolved (either confirmed working or the fallback applied).
- No subprocess or filesystem work was added to the plugin render path.
