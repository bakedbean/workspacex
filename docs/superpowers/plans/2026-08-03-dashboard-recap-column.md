# Dashboard Condensed-Recap Column Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the dashboard flex column's status-message/last-agent-message content with `<status token> · <goal-short> · <state-short> · <next-short>`, where the short forms are agent-authored keyword distillations stored on `workspace_recap`.

**Architecture:** Three nullable columns are added to the `workspace_recap` table (migration guard: `add_column_if_missing`, since `migrate()` re-runs every launch). `wsx recap set` gains `--goal-short/--state-short/--next-short`. The composer `column_content::row_column` is reworked to return a structured `RowColumn { token, reported, body }`; the renderer `row.rs` does greedy width-fitting of the segments (drop `next`, then `state`; only `goal` truncates). Agent doctrine and the wsx skill teach agents the convention.

**Tech Stack:** Rust, rusqlite, ratatui. Spec: `docs/superpowers/specs/2026-08-03-dashboard-recap-column-design.md`.

## Global Constraints

- The spec says "migration `SCHEMA_V18`" but V18 (`scm_cache`) and V19 already exist — the new block is **`if v < 20`**. Task 1 fixes the spec text.
- `migrate()` re-runs ALL blocks every launch (`SCHEMA_V1` resets `user_version` to 1). Every `ALTER TABLE` goes through the existing `add_column_if_missing` helper — never a bare `ALTER TABLE`.
- Full-field clip length when a short form is missing: **32 chars** (via `crate::ui::text::truncate`, which replaces the last kept char with `…`).
- Derived status token vocabulary: `asking` (Question), `stalled`, `waiting`, `thinking`, `done` (Complete), `idle`. Fresh-push token vocabulary: `working`/`waiting`/`blocked`/`done`/`busy` via `ReportedState::as_str()`.
- Doctrine length guidance (docs only, not enforced): goal-short ≤ ~40 chars, state-short/next-short ≤ ~24.
- CI gates are separate: run `cargo fmt --check`, `cargo clippy --all-targets`, `cargo test` before claiming done. `click_chip_auto_spawns_session_when_missing` is a known flaky PTY-timing test — a solo failure there is not caused by this work; rerun it.
- `src/menubar/` is macOS-gated: Linux `cargo check` won't compile-verify edits there. Make the mechanical edits anyway.

---

### Task 1: Schema V20 + store short-form fields

**Files:**
- Modify: `src/data/schema.rs` (migration ladder ends at `v < 19`, ~line 137)
- Modify: `src/data/store.rs:96-102` (`WorkspaceRecap`), tests ~line 500
- Modify: `src/data/recap.rs` (all SQL + tests)
- Modify: `src/cli.rs:1949-1953` (call site gets `None` padding — flags come in Task 2)
- Modify: `src/menubar/pm.rs`, `src/menubar/plugin.rs:411`, `src/ui/pm_pane.rs:505,731` (struct-literal fixups)
- Modify: `docs/superpowers/specs/2026-08-03-dashboard-recap-column-design.md` (V18 → V20)

**Interfaces:**
- Produces: `WorkspaceRecap { goal, state, next, goal_short, state_short, next_short: Option<String>, updated_at: i64 }` with `#[derive(Default)]`; `Store::set_workspace_recap(id, goal, state, next, goal_short, state_short, next_short)` — seven args, all field args `Option<&str>`, partial upsert semantics unchanged.

- [ ] **Step 1: Write the failing store tests** — append to `mod tests` in `src/data/recap.rs`:

```rust
#[test]
fn short_forms_round_trip() {
    let (store, ws) = store_with_workspace();
    store
        .set_workspace_recap(
            ws,
            Some("Audit all V2 invoices for the CV-04964 drift bug"),
            None,
            None,
            Some("Audit V2 invoices, CV-04964"),
            Some("3/12 done"),
            Some("fix drift calc"),
        )
        .unwrap();
    let got = store.workspace_recap(ws).unwrap().unwrap();
    assert_eq!(got.goal_short.as_deref(), Some("Audit V2 invoices, CV-04964"));
    assert_eq!(got.state_short.as_deref(), Some("3/12 done"));
    assert_eq!(got.next_short.as_deref(), Some("fix drift calc"));
}

#[test]
fn partial_update_preserves_short_forms() {
    let (store, ws) = store_with_workspace();
    store
        .set_workspace_recap(ws, Some("g"), None, None, Some("g-short"), None, None)
        .unwrap();
    store
        .set_workspace_recap(ws, None, Some("tests green"), None, None, Some("s-short"), None)
        .unwrap();
    let got = store.workspace_recap(ws).unwrap().unwrap();
    assert_eq!(got.goal_short.as_deref(), Some("g-short"), "goal_short must survive");
    assert_eq!(got.state_short.as_deref(), Some("s-short"));
    assert_eq!(got.next_short, None);
    // shorts also come back through the bulk read
    let map = store.all_workspace_recaps().unwrap();
    assert_eq!(map.get(&ws).unwrap().state_short.as_deref(), Some("s-short"));
}
```

And the migration-idempotency test next to `migrate_v16_is_idempotent` in `src/data/store.rs` tests:

```rust
#[test]
fn migrate_v20_recap_short_columns_idempotent() {
    let store = Store::open_in_memory().unwrap();
    store.migrate_for_test().unwrap(); // re-run must not error
    let n: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM pragma_table_info('workspace_recap') \
             WHERE name IN ('goal_short','state_short','next_short')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 3, "all three short-form columns must exist");
}
```

Existing `set_workspace_recap` call sites in `recap.rs` tests won't compile until Step 3 — that's the expected failure mode for this cycle (compile error, not assert failure).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p workspacex --lib data::recap 2>&1 | tail -20`
Expected: compile error — `set_workspace_recap` takes 4 args / unknown fields `goal_short`.
(If the crate name differs, use plain `cargo test --lib data::recap`.)

- [ ] **Step 3: Implement schema + store changes**

In `src/data/schema.rs`, after the `if v < 19 { ... }` block (line ~141):

```rust
        if v < 20 {
            self.add_column_if_missing("workspace_recap", "goal_short", "goal_short TEXT")?;
            self.add_column_if_missing("workspace_recap", "state_short", "state_short TEXT")?;
            self.add_column_if_missing("workspace_recap", "next_short", "next_short TEXT")?;
            self.conn().execute("PRAGMA user_version = 20", [])?;
        }
```

In `src/data/store.rs`, extend the struct (keep existing derives, add `Default`):

```rust
/// A row from the `workspace_recap` table: the goal / state / next digest a
/// workspace's agent maintains via `wsx recap set`. The `*_short` fields are
/// agent-authored keyword distillations rendered by the dashboard row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceRecap {
    pub goal: Option<String>,
    pub state: Option<String>,
    pub next: Option<String>,
    pub goal_short: Option<String>,
    pub state_short: Option<String>,
    pub next_short: Option<String>,
    pub updated_at: i64,
}
```

In `src/data/recap.rs`, extend all four queries:

```rust
    pub fn set_workspace_recap(
        &self,
        id: WorkspaceId,
        goal: Option<&str>,
        state: Option<&str>,
        next: Option<&str>,
        goal_short: Option<&str>,
        state_short: Option<&str>,
        next_short: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO workspace_recap \
                 (workspace_id, goal, state, next, goal_short, state_short, next_short, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(workspace_id) DO UPDATE SET \
                 goal        = COALESCE(excluded.goal, workspace_recap.goal), \
                 state       = COALESCE(excluded.state, workspace_recap.state), \
                 next        = COALESCE(excluded.next, workspace_recap.next), \
                 goal_short  = COALESCE(excluded.goal_short, workspace_recap.goal_short), \
                 state_short = COALESCE(excluded.state_short, workspace_recap.state_short), \
                 next_short  = COALESCE(excluded.next_short, workspace_recap.next_short), \
                 updated_at  = excluded.updated_at",
            rusqlite::params![id.0, goal, state, next, goal_short, state_short, next_short, now_ms()],
        )?;
        Ok(())
    }
```

`workspace_recap` SELECT becomes `SELECT goal, state, next, goal_short, state_short, next_short, updated_at …`; `row_to_recap` maps indices 0-6. `all_workspace_recaps` SELECT becomes `SELECT workspace_id, goal, state, next, goal_short, state_short, next_short, updated_at …` with the closure mapping indices 1-7.

- [ ] **Step 4: Fix the callers that now fail to compile**
  - `src/cli.rs:1951`: `store.set_workspace_recap(ws.id, goal.as_deref(), state.as_deref(), next.as_deref(), None, None, None)?;`
  - Existing tests in `src/data/recap.rs` (`recap_round_trips`, `partial_update_preserves_other_fields_and_bumps_updated_at`, `clear_and_all_recaps`, `recap_cascade_deletes_with_workspace`): append `, None, None, None` to each `set_workspace_recap` call.
  - Struct literals: run `grep -rn "WorkspaceRecap {" src/` and append `..Default::default()` as the final entry in every literal outside `src/data/recap.rs` row-mappers (expected sites: `src/menubar/pm.rs` ~lines 233, 321, 338, 358, 487, 545, 575; `src/menubar/plugin.rs:411`; `src/ui/pm_pane.rs:505, 731`). In `recap.rs`'s `row_to_recap`/`all_workspace_recaps` closures, map the columns explicitly instead.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib data:: 2>&1 | tail -5`
Expected: PASS including the two new tests and `migrate_v20_recap_short_columns_idempotent`.

- [ ] **Step 6: Fix the spec's migration number**

In `docs/superpowers/specs/2026-08-03-dashboard-recap-column-design.md`, change "New migration block `SCHEMA_V18`" to "New migration block (`if v < 20`; V18/V19 were already taken by scm_cache)".

- [ ] **Step 7: Commit**

```bash
git add src/data/schema.rs src/data/store.rs src/data/recap.rs src/cli.rs src/menubar/pm.rs src/menubar/plugin.rs src/ui/pm_pane.rs docs/superpowers/specs/2026-08-03-dashboard-recap-column-design.md
git commit -m "feat(store): short-form recap fields (goal/state/next_short, migration v20)"
```

---

### Task 2: CLI `--goal-short/--state-short/--next-short`

**Files:**
- Modify: `src/cli.rs:510-517` (`CliAction::RecapSet`), `:1291-1330` (`parse_recap`), `:1949-1964` (dispatch + show), `:209-226` (help `GroupInfo`), tests ~line 3140
- Test: `src/cli.rs` inline `mod tests`

**Interfaces:**
- Consumes: `Store::set_workspace_recap(id, goal, state, next, goal_short, state_short, next_short)` from Task 1.
- Produces: `CliAction::RecapSet { goal, state, next, goal_short, state_short, next_short: Option<String> }`.

- [ ] **Step 1: Write the failing parse tests** — append near the existing recap parse tests (~line 3140):

```rust
#[test]
fn parses_recap_set_short_forms() {
    let a = parse(&[
        "recap", "set",
        "--goal-short", "Audit V2 invoices, CV-04964",
        "--state-short", "3/12 done",
        "--next-short", "fix drift calc",
    ])
    .unwrap();
    match a {
        CliAction::RecapSet { goal, goal_short, state_short, next_short, .. } => {
            assert_eq!(goal, None);
            assert_eq!(goal_short.as_deref(), Some("Audit V2 invoices, CV-04964"));
            assert_eq!(state_short.as_deref(), Some("3/12 done"));
            assert_eq!(next_short.as_deref(), Some("fix drift calc"));
        }
        other => panic!("expected RecapSet, got {other:?}"),
    }
}

#[test]
fn recap_set_short_flag_alone_satisfies_at_least_one() {
    assert!(parse(&["recap", "set", "--goal-short", "x"]).is_ok());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib cli::tests::parses_recap_set_short_forms 2>&1 | tail -10`
Expected: compile error (unknown fields on `RecapSet`).

- [ ] **Step 3: Implement**

`CliAction::RecapSet` gains `goal_short`, `state_short`, `next_short: Option<String>`. In `parse_recap`:

```rust
        Some("set") => {
            let mut goal = None;
            let mut state = None;
            let mut next = None;
            let mut goal_short = None;
            let mut state_short = None;
            let mut next_short = None;
            while let Some(arg) = it.next() {
                let slot = match arg.as_str() {
                    "--goal" => &mut goal,
                    "--state" => &mut state,
                    "--next" => &mut next,
                    "--goal-short" => &mut goal_short,
                    "--state-short" => &mut state_short,
                    "--next-short" => &mut next_short,
                    _ => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unexpected argument: {arg}"),
                        });
                    }
                };
                *slot = Some(it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: format!("{arg} requires a value"),
                })?);
            }
            if [&goal, &state, &next, &goal_short, &state_short, &next_short]
                .iter()
                .all(|o| o.is_none())
            {
                return Err(Error::Usage {
                    group: None,
                    msg: "usage: wsx recap set [--goal|--state|--next <text>] \
                          [--goal-short|--state-short|--next-short <text>] (at least one)"
                        .into(),
                });
            }
            Ok(CliAction::RecapSet { goal, state, next, goal_short, state_short, next_short })
        }
```

Dispatch (~line 1949): destructure the new fields and pass all six `as_deref()` to `set_workspace_recap`. `RecapShow` prints:

```rust
        CliAction::RecapShow => {
            let ws = resolve_current_workspace(&store)?;
            match store.workspace_recap(ws.id)? {
                Some(r) => {
                    println!("goal:        {}", r.goal.as_deref().unwrap_or("-"));
                    println!("state:       {}", r.state.as_deref().unwrap_or("-"));
                    println!("next:        {}", r.next.as_deref().unwrap_or("-"));
                    println!("goal-short:  {}", r.goal_short.as_deref().unwrap_or("-"));
                    println!("state-short: {}", r.state_short.as_deref().unwrap_or("-"));
                    println!("next-short:  {}", r.next_short.as_deref().unwrap_or("-"));
                }
                None => println!("no recap set"),
            }
        }
```

Help `GroupInfo` (line ~213):

```rust
            CmdInfo {
                usage: "set [--goal|--state|--next <text>] [--goal-short|--state-short|--next-short <text>]",
                blurb: "Update recap fields (partial; at least one flag). *-short: keyword \
                        distillation for the dashboard row — identifiers, ticket/PR numbers, \
                        no filler (e.g. \"Audit V2 invoices, CV-04964, bug from #2835\")",
            },
```

Fix the existing match arms in tests that destructure `RecapSet { goal, state, next }` — add `, ..` to each pattern (`parses_recap_set_with_all_flags`, `parses_recap_set_partial`, and the match at ~line 3153).

- [ ] **Step 4: Run tests**

Run: `cargo test --lib cli:: 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): recap short-form flags and show output"
```

---

### Task 3: Composer + renderer — token · segments column

This is the core feature commit: `column_content.rs` gets a structured `RowColumn`, `row.rs` does the width fitting, `render.rs` feeds recap + reported state in. All in one commit because the type change is atomic across these files.

**Files:**
- Modify: `src/ui/dashboard/column_content.rs` (rework + rewrite tests)
- Modify: `src/ui/dashboard/row.rs:270-306` (flex column render), tests
- Modify: `src/app/render.rs:121-127` (call site)
- Modify: `src/ui/dashboard/by_repo.rs:184`, `src/ui/dashboard/by_attention.rs:248,420-424`, `src/ui/dashboard/tests.rs:56` (fixture literals)

**Interfaces:**
- Consumes: `WorkspaceRecap` (Task 1); `crate::data::store::ReportedStatus` / `ReportedState::as_str()`; `crate::ui::pm_pane::RECAP_STALE_SLACK_MS`; `crate::ui::text::truncate`.
- Produces:

```rust
pub struct RowColumn { pub token: String, pub reported: bool, pub body: ColumnBody }
pub enum ColumnBody {
    Recap { segments: Vec<String>, stale: bool },
    Fallback { text: String, emphasis: ColumnEmphasis },
    Empty,
}
pub enum ColumnEmphasis { Dim, Status, Warn }  // `Reported` variant removed
pub fn row_column(
    status: Status,
    events: Option<&WorkspaceEvents>,
    now_ms: i64,
    reported: Option<&ReportedStatus>,
    recap: Option<&WorkspaceRecap>,
) -> RowColumn
```

  and in `row.rs`: `fn fit_segments(segments: &[String], avail: usize) -> String`. `RowInputs.column` stays `Option<RowColumn>` (`None` still renders the em-dash; `render.rs` always passes `Some`).

- [ ] **Step 1: Write the new composer tests.** Replace the test module of `column_content.rs` — keep every existing test that exercises the *fallback derivations* (question/stalled/thinking/complete/idle arms) but adapt assertions to the new shape, and add recap/token tests. Representative new tests (write all of these; adapt the kept ones to match):

```rust
    fn recap_with(
        goal: Option<&str>, goal_short: Option<&str>,
        state_short: Option<&str>, next_short: Option<&str>,
        updated_at: i64,
    ) -> WorkspaceRecap {
        WorkspaceRecap {
            goal: goal.map(String::from),
            goal_short: goal_short.map(String::from),
            state_short: state_short.map(String::from),
            next_short: next_short.map(String::from),
            updated_at,
            ..Default::default()
        }
    }

    fn reported(state: ReportedState) -> ReportedStatus {
        ReportedStatus {
            state,
            message: Some("ignored by the column now".into()),
            source: "model".into(),
            reported_at: 0,
        }
    }

    #[test]
    fn token_derives_from_status_when_no_push() {
        let c = row_column(Status::Question, Some(&evt()), 0, None, None);
        assert_eq!(c.token, "asking");
        assert!(!c.reported);
        let c = row_column(Status::Complete, Some(&evt()), 0, None, None);
        assert_eq!(c.token, "done");
    }

    #[test]
    fn fresh_push_sets_token_and_reported_flag() {
        let r = reported(ReportedState::Blocked);
        let c = row_column(Status::Waiting, Some(&evt()), 0, Some(&r), None);
        assert_eq!(c.token, "blocked");
        assert!(c.reported);
    }

    #[test]
    fn pushed_message_text_no_longer_appears() {
        let r = reported(ReportedState::Working);
        let c = row_column(Status::Waiting, None, 0, Some(&r), None);
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn recap_prefers_short_forms_in_order() {
        let rc = recap_with(None, Some("Audit V2 #2835"), Some("3/12 done"), Some("fix drift"), 0);
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, Some(&rc));
        match c.body {
            ColumnBody::Recap { segments, stale } => {
                assert_eq!(segments, vec!["Audit V2 #2835", "3/12 done", "fix drift"]);
                assert!(!stale);
            }
            other => panic!("expected Recap, got {other:?}"),
        }
    }

    #[test]
    fn missing_short_falls_back_to_clipped_full_field() {
        let long = "Audit all V2 invoices auto-issued today for the CV-04964 amount-drift bug";
        let rc = recap_with(Some(long), None, Some("3/12 done"), None, 0);
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, Some(&rc));
        match c.body {
            ColumnBody::Recap { segments, .. } => {
                assert_eq!(segments.len(), 2, "absent next is skipped, not placeholder'd");
                assert_eq!(segments[0].chars().count(), 32);
                assert!(segments[0].ends_with('…'));
            }
            other => panic!("expected Recap, got {other:?}"),
        }
    }

    #[test]
    fn all_empty_recap_behaves_as_no_recap() {
        let rc = recap_with(None, None, None, None, 0);
        let e = WorkspaceEvents {
            first_user_text: Some("migrate auth".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Idle, Some(&e), 0, None, Some(&rc));
        assert!(matches!(c.body, ColumnBody::Fallback { .. }));
    }

    #[test]
    fn recap_stale_when_activity_outruns_updated_at() {
        let rc = recap_with(None, Some("g"), None, None, 1_000);
        let e = WorkspaceEvents {
            last_log_activity_ms: 1_000 + crate::ui::pm_pane::RECAP_STALE_SLACK_MS + 1,
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Waiting, Some(&e), 0, None, Some(&rc));
        assert!(matches!(c.body, ColumnBody::Recap { stale: true, .. }));
    }

    #[test]
    fn question_fallback_drops_asking_prefix() {
        // Token already says "asking"; the fallback body is the bare topic.
        let mut e = evt();
        e.pending_tool_uses.insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        e.pending_question_text = Some("Auth method".into());
        let c = row_column(Status::Question, Some(&e), 10_000, None, None);
        assert_eq!(c.token, "asking");
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "Auth method");
                assert_eq!(emphasis, ColumnEmphasis::Status);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn stalled_fallback_is_quiet_detail_only() {
        let e = WorkspaceEvents { last_log_activity_ms: 1, ..WorkspaceEvents::default() };
        let c = row_column(Status::Stalled, Some(&e), 240_000, None, None);
        assert_eq!(c.token, "stalled");
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "3m quiet");
                assert_eq!(emphasis, ColumnEmphasis::Warn);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn no_events_no_recap_is_token_only() {
        let c = row_column(Status::Idle, None, 0, None, None);
        assert_eq!(c.token, "idle");
        assert!(matches!(c.body, ColumnBody::Empty));
    }
```

Adaptation guide for the kept fallback tests: `thinking_shows_tool_trace_dim` and friends now assert on `ColumnBody::Fallback { text, emphasis }` instead of `c.text`/`c.emphasis`; `thinking_with_no_tools_yet_shows_ellipsis_label` becomes "thinking with no tools yet → `ColumnBody::Empty`" (the `{label}…` filler is gone — the token carries the state); `reported_message_overrides_heuristic_recap` / `reported_message_shows_even_without_events` / `empty_reported_message_falls_back_to_heuristic` are superseded by the new reported tests above and are deleted.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::dashboard::column_content 2>&1 | tail -10`
Expected: compile error (`RowColumn` shape, `row_column` arity).

- [ ] **Step 3: Rework `column_content.rs`**

```rust
use crate::data::store::{ReportedStatus, WorkspaceRecap};
use crate::ui::pm_pane::RECAP_STALE_SLACK_MS;
use crate::ui::text::truncate;

/// Chars a full recap field is clipped to when its short form is absent.
pub const RECAP_FIELD_CLIP: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowColumn {
    /// Status word rendered first, always present: a fresh agent-pushed state
    /// (`working`/`blocked`/…) or the derived label (`asking`/`stalled`/…).
    pub token: String,
    /// Token came from a fresh push — the renderer uses the `▸ ` prefix.
    pub reported: bool,
    pub body: ColumnBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnBody {
    /// Agent-authored recap segments (goal/state/next, short forms preferred),
    /// greedy-fitted by the renderer. `stale`: activity outran `updated_at`.
    Recap { segments: Vec<String>, stale: bool },
    /// No recap — the pre-recap heuristic text (question topic, tool trace,
    /// last turn text…), already stripped of the status word the token carries.
    Fallback { text: String, emphasis: ColumnEmphasis },
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnEmphasis {
    Dim,
    Status,
    Warn,
}

fn token_for(status: Status) -> &'static str {
    match status {
        Status::Question => "asking",
        Status::Complete => "done",
        other => other.label(),
    }
}

pub fn row_column(
    status: Status,
    events: Option<&WorkspaceEvents>,
    now_ms: i64,
    reported: Option<&ReportedStatus>,
    recap: Option<&WorkspaceRecap>,
) -> RowColumn {
    let (token, is_reported) = match reported {
        Some(r) => (r.state.as_str().to_string(), true),
        None => (token_for(status).to_string(), false),
    };
    let segments = recap.map(recap_segments).unwrap_or_default();
    let body = if !segments.is_empty() {
        let last_activity = events.map(|e| e.last_log_activity_ms).unwrap_or(0);
        let stale = recap
            .map(|r| last_activity > r.updated_at + RECAP_STALE_SLACK_MS)
            .unwrap_or(false);
        ColumnBody::Recap { segments, stale }
    } else {
        match fallback_text(status, events, now_ms) {
            Some((text, emphasis)) => ColumnBody::Fallback { text, emphasis },
            None => ColumnBody::Empty,
        }
    };
    RowColumn { token, reported: is_reported, body }
}

/// Short form preferred, full field clipped to `RECAP_FIELD_CLIP` otherwise,
/// absent fields skipped. Order: goal, state, next.
fn recap_segments(r: &WorkspaceRecap) -> Vec<String> {
    [
        (&r.goal_short, &r.goal),
        (&r.state_short, &r.state),
        (&r.next_short, &r.next),
    ]
    .into_iter()
    .filter_map(|(short, full)| {
        non_empty_trimmed(short.as_deref())
            .map(collapse_ws)
            .or_else(|| {
                non_empty_trimmed(full.as_deref())
                    .map(|f| truncate(&collapse_ws(f), RECAP_FIELD_CLIP))
            })
    })
    .collect()
}

/// The pre-recap heuristic body, minus the status word (the token carries it):
/// the old `Question` arm's "asking: X" becomes "X", the old `Stalled` arm's
/// "stalled · 3m quiet" becomes "3m quiet", and the `{label}…` fillers vanish.
fn fallback_text(
    status: Status,
    events: Option<&WorkspaceEvents>,
    now_ms: i64,
) -> Option<(String, ColumnEmphasis)> {
    let evt = events?;
    match status {
        Status::Question => {
            let body = match evt.pending_question_tool() {
                Some("ExitPlanMode") => Some("review plan".to_string()),
                Some(_) => non_empty_trimmed(evt.pending_question_text.as_deref())
                    .map(collapse_ws),
                None => evt
                    .pending_permission_tool(now_ms, 3_000)
                    .map(|(n, _)| format!("awaiting: {n}")),
            };
            body.map(|t| (t, ColumnEmphasis::Status))
        }
        Status::Stalled => {
            if evt.last_log_activity_ms > 0 {
                let quiet_secs =
                    now_ms.saturating_sub(evt.last_log_activity_ms).max(0) as u64 / 1000;
                Some((
                    format!("{} quiet", format_ago_short(Some(quiet_secs))),
                    ColumnEmphasis::Warn,
                ))
            } else {
                None
            }
        }
        Status::Thinking | Status::Waiting => {
            let trace = format_tool_trace(&evt.tool_use_counts);
            let live = non_empty_trimmed(evt.current_action.as_deref());
            let text = match (trace.is_empty(), live) {
                (false, Some(l)) => format!("{trace} · {l}"),
                (false, None) => trace,
                (true, Some(l)) => l.to_string(),
                (true, None) => return None,
            };
            Some((text, ColumnEmphasis::Dim))
        }
        Status::Complete => non_empty_trimmed(evt.last_completed_turn_text.as_deref())
            .or_else(|| non_empty_trimmed(evt.first_user_text.as_deref()))
            .map(|t| (collapse_ws(t), ColumnEmphasis::Dim)),
        Status::Idle => non_empty_trimmed(evt.first_user_text.as_deref())
            .map(|t| (collapse_ws(t), ColumnEmphasis::Dim)),
    }
}
```

Keep `format_state_line`, `format_ago_short`, `format_tool_trace`, `non_empty_trimmed`, `collapse_ws`, `plural` unchanged — `format_state_line` is still consumed by `src/detail_modules/session_summary.rs`. Change `collapse_ws` usage sites as shown (`.map(collapse_ws)` works because it takes `&str`; if the compiler objects over the closure form, use `.map(|s| collapse_ws(s))`).

- [ ] **Step 4: Rework the row renderer.** In `src/ui/dashboard/row.rs`, replace the flex-column block (lines 283-306):

```rust
    if let Some(col) = inputs.column.as_ref() {
        let prefix = if col.reported { "▸ " } else { "└ " };
        let body_width = message_width.saturating_sub(prefix.chars().count());
        spans.push(Span::styled(
            prefix.to_string(),
            theme.status_style(inputs.status),
        ));
        let token = truncate(&col.token, body_width);
        let token_style = if inputs.status == Status::Stalled && !col.reported {
            theme.warn_style()
        } else {
            theme.status_style(inputs.status)
        };
        let mut used = token.chars().count();
        spans.push(Span::styled(token, token_style));
        let avail = body_width.saturating_sub(used);
        let (rest, rest_style) = match &col.body {
            ColumnBody::Recap { segments, stale } => {
                let text = fit_segments(segments, avail);
                let style = if *stale {
                    theme.dim_style().add_modifier(Modifier::DIM)
                } else {
                    theme.dim_style()
                };
                (text, style)
            }
            ColumnBody::Fallback { text, emphasis } => {
                let sep_len = SEG_SEP.chars().count();
                let fitted = if avail > sep_len + 1 {
                    format!("{SEG_SEP}{}", truncate(text, avail - sep_len))
                } else {
                    String::new()
                };
                let style = match emphasis {
                    ColumnEmphasis::Dim => theme.dim_style(),
                    ColumnEmphasis::Status => theme.status_style(inputs.status),
                    ColumnEmphasis::Warn => theme.warn_style(),
                };
                (fitted, style)
            }
            ColumnBody::Empty => (String::new(), theme.dim_style()),
        };
        used += rest.chars().count();
        if !rest.is_empty() {
            spans.push(Span::styled(rest, rest_style));
        }
        spans.push(Span::styled(
            " ".repeat(body_width.saturating_sub(used)),
            theme.dim_style(),
        ));
    } else {
        let body = truncate_pad("—", message_width);
        spans.push(Span::styled(body, theme.dim_style()));
    }
```

Add the separator + fitting helper next to `right_pad`:

```rust
const SEG_SEP: &str = " · ";

/// Greedy segment fitting for the recap body. The first segment (goal) is
/// always included, truncated to what remains; later segments (state, next)
/// are appended only when they fit whole — a segment that doesn't fit is
/// dropped along with everything after it.
fn fit_segments(segments: &[String], avail: usize) -> String {
    let sep_len = SEG_SEP.chars().count();
    let mut out = String::new();
    let mut used = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        if i == 0 {
            if avail <= sep_len + 1 {
                break;
            }
            let t = truncate(seg, avail - sep_len);
            used = sep_len + t.chars().count();
            out.push_str(SEG_SEP);
            out.push_str(&t);
        } else {
            let seg_len = seg.chars().count();
            if used + sep_len + seg_len <= avail {
                out.push_str(SEG_SEP);
                out.push_str(seg);
                used += sep_len + seg_len;
            } else {
                break;
            }
        }
    }
    out
}
```

Update the import at row.rs:19 to `use crate::ui::dashboard::column_content::{ColumnBody, ColumnEmphasis, RowColumn};`. `Status` needs `PartialEq` comparison (`inputs.status == Status::Stalled`) — it already derives `PartialEq`.

- [ ] **Step 5: Update the call site and fixtures.**

`src/app/render.rs:121-127`:

```rust
                        column: Some(crate::ui::dashboard::column_content::row_column(
                            status,
                            app.workspace_events.get(&ws.id),
                            now_ms,
                            app.fresh_reported_status(ws.id),
                            app.recaps.get(&ws.id),
                        )),
```

Fixture literals in `src/ui/dashboard/by_repo.rs:184`, `by_attention.rs:248` and `:420-424`, `tests.rs:56` — replace each `RowColumn { text: t, emphasis: ColumnEmphasis::Dim }` with:

```rust
                        column: w.last_message.clone().map(|t| RowColumn {
                            token: "idle".to_string(),
                            reported: false,
                            body: ColumnBody::Fallback {
                                text: t,
                                emphasis: ColumnEmphasis::Dim,
                            },
                        }),
```

(adjust the surrounding expression per site; import `ColumnBody` in each test module). Update the `RowColumn` literals in `row.rs`'s own tests (~lines 367, 578-608) the same way, then add renderer tests:

```rust
    #[test]
    fn fit_segments_drops_then_truncates() {
        let segs = vec!["goal seg".to_string(), "state".to_string(), "next".to_string()];
        // everything fits: " · goal seg · state · next" = 26 chars
        assert_eq!(fit_segments(&segs, 26), " · goal seg · state · next");
        // next no longer fits whole → dropped
        assert_eq!(fit_segments(&segs, 25), " · goal seg · state");
        // state no longer fits whole → dropped
        assert_eq!(fit_segments(&segs, 18), " · goal seg");
        // goal itself doesn't fit → truncated with …
        assert_eq!(fit_segments(&segs, 9), " · goal …");
        // no room for anything meaningful
        assert_eq!(fit_segments(&segs, 4), "");
    }

    #[test]
    fn reported_token_gets_pointer_prefix() {
        let mut inputs = base();
        inputs.column = Some(RowColumn {
            token: "working".to_string(),
            reported: true,
            body: ColumnBody::Empty,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &Theme::default(), 160);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("▸ working"), "got: {text}");
    }

    #[test]
    fn recap_segments_render_after_token() {
        let mut inputs = base();
        inputs.column = Some(RowColumn {
            token: "working".to_string(),
            reported: false,
            body: ColumnBody::Recap {
                segments: vec!["Audit V2 #2835".to_string(), "3/12 done".to_string()],
                stale: false,
            },
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &Theme::default(), 160);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("working · Audit V2 #2835 · 3/12 done"), "got: {text}");
    }

    #[test]
    fn stale_recap_body_renders_extra_dim() {
        let mut inputs = base();
        inputs.column = Some(RowColumn {
            token: "idle".to_string(),
            reported: false,
            body: ColumnBody::Recap {
                segments: vec!["Audit V2 #2835".to_string()],
                stale: true,
            },
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &Theme::default(), 160);
        let seg_span = line
            .spans
            .iter()
            .find(|s| s.content.contains("Audit V2"))
            .expect("segment span present");
        assert!(seg_span.style.add_modifier.contains(Modifier::DIM));
    }
```

(Adapt `base()`/`Theme::default()` construction to whatever the existing row tests use — follow the surrounding test code, not this sketch, for scaffolding.)

- [ ] **Step 6: Run the full dashboard test slice**

Run: `cargo test --lib ui::dashboard 2>&1 | tail -5` then `cargo test --lib 2>&1 | tail -5`
Expected: PASS. If `app::render` or `detail_modules` fail to compile, chase remaining call sites of the old `RowColumn` shape (`grep -rn "emphasis:" src/ui src/app`).

- [ ] **Step 7: Commit**

```bash
git add src/ui/dashboard/column_content.rs src/ui/dashboard/row.rs src/app/render.rs src/ui/dashboard/by_repo.rs src/ui/dashboard/by_attention.rs src/ui/dashboard/tests.rs
git commit -m "feat(tui): dashboard flex column shows status token + condensed recap"
```

---

### Task 4: Doctrine + wsx skill teach the convention

**Files:**
- Modify: `src/agent/doctrine.rs:38-42` (`CLAUSE_RECAP`), test module
- Modify: `skills/wsx/SKILL.md` "Maintaining the workspace recap" section (~lines 67-77)

**Interfaces:**
- Consumes: CLI flags from Task 2 (doctrine text must match real flag names exactly).

- [ ] **Step 1: Write the failing doctrine test**

```rust
    #[test]
    fn doctrine_mentions_recap_short_forms() {
        for agent in [AgentKind::Claude, AgentKind::Pi, AgentKind::Hermes, AgentKind::Codex] {
            let d = process_doctrine(agent).to_lowercase();
            assert!(
                d.contains("--goal-short"),
                "doctrine must teach {agent:?} the short-form flags: {d}"
            );
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib agent::doctrine::tests::doctrine_mentions_recap_short_forms 2>&1 | tail -5`
Expected: FAIL (assertion, not compile error).

- [ ] **Step 3: Update the clause**

```rust
const CLAUSE_RECAP: &str = "- Maintain the workspace recap with `wsx recap set`: run \
    `wsx recap set --goal \"<one line>\"` once you understand the task's scope, and \
    update `--state \"<one line>\"` and `--next \"<one line>\"` whenever you set status \
    and whenever you end a turn with the task unfinished. Alongside each full field, \
    keep keyword short forms for the dashboard row: `--goal-short` (≤40 chars), \
    `--state-short` and `--next-short` (≤24 chars) — identifiers and ticket/PR numbers, \
    no filler. Example: --goal \"Audit all V2 invoices auto-issued today for the \
    CV-04964 amount-drift bug fixed in PR #2835\" --goal-short \"Audit V2 invoices, \
    CV-04964, bug #2835\". The project-manager digest renders the full lines; the \
    dashboard row renders the short forms.";
```

- [ ] **Step 4: Update `skills/wsx/SKILL.md`** — replace the recap section's code block and trailing line with:

```sh
wsx recap set --goal "cookie expiry bug from #42" --goal-short "cookie expiry, #42"   # once, when scope is clear
wsx recap set --state "tests added but failing" --state-short "tests failing" \
              --next "debug session token regex" --next-short "debug token regex"
wsx recap show
```

Fields update independently; set `--goal`/`--goal-short` once and refresh the state/next pairs as work progresses. Short forms are keyword distillations for the dashboard row — identifiers and ticket/PR numbers, no filler; aim for ≤40 chars (goal) / ≤24 chars (state, next).

- [ ] **Step 5: Run tests, then commit**

Run: `cargo test --lib agent::doctrine 2>&1 | tail -5`
Expected: PASS.

```bash
git add src/agent/doctrine.rs skills/wsx/SKILL.md
git commit -m "docs(doctrine): teach agents the recap short-form convention"
```

---

### Task 5: Full verification pass

- [ ] **Step 1: Run all three CI gates**

```bash
cargo fmt --check
cargo clippy --all-targets 2>&1 | tail -5
cargo test 2>&1 | tail -10
```

Expected: all clean. `cargo fmt --check` failures: run `cargo fmt` and amend the offending commit if obvious, else add a `style: rustfmt` commit. A solo failure of `click_chip_auto_spawns_session_when_missing` is the known flaky PTY test — rerun it before investigating.

- [ ] **Step 2: Smoke-test the CLI against a real workspace**

```bash
cargo run --quiet -- recap set --goal-short "Audit V2 invoices, CV-04964" --state-short "3/12 done"
cargo run --quiet -- recap show
```

Expected: `recap updated`, then `show` prints the short forms.

- [ ] **Step 3: Commit any stragglers; done**
