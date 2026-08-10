# Multi-Agent Dashboard Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show one colored bar per *live* agent instance on each dashboard workspace row, so a workspace running peer agents is visibly distinct from one running only its primary.

**Architecture:** The dashboard row's leftmost column is currently a fixed 1-char bar (`AGENT_WIDTH`) colored by the primary agent's kind. It becomes a variable-width, right-aligned strip: one `▎` per live agent, primary rightmost. The width is derived per frame as `clamp(max live-agent count across visible rows, 1, 4)`, computed where both the renderer and the PR-chip hit-tester can see it. Liveness comes from the in-memory session map; the instance roster comes from a cache refilled in `App::refresh()`.

**Tech Stack:** Rust, ratatui (TUI rendering), rusqlite (SQLite store), portable-pty (agent sessions).

**Spec:** `docs/superpowers/specs/2026-08-09-multi-agent-dashboard-indicator-design.md`

## Global Constraints

- **Single-width glyphs only.** `display_width` in `row.rs` is literally `s.chars().count()`. Any double-width glyph misaligns every column to its right. The strip uses `▎` (U+258E) and ASCII `+`.
- **No nerd-font gating.** `▎` and `+` are plain Unicode/ASCII, matching how the existing agent bar is drawn. Do not add a `nerd_fonts` branch to the strip.
- **Strip cap is 4.** `MAX_AGENT_WIDTH = 4`. Five live agents is reachable with one keystroke (the agents-panel `a` key adds all four kinds at once, `input.rs:1877-1883`), so overflow is real and must render.
- **The primary bar is never liveness-gated.** It renders in its kind color whether or not a session is running. Only *peer* bars require `SessionStatus::Running`. This keeps the change additive — a never-attached workspace looks exactly as it does today.
- **No schema migration.** No new tables or columns. Do not bump the `user_version` assertion in the `migration_v12` test.
- **CI gates:** `mise exec rust@1.95.0 -- cargo fmt --all --check`, `cargo clippy`, `cargo test`. Ambient local rustfmt differs from the CI-pinned 1.95.0 and will false-pass — always use the `mise exec` form.

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src/data/agents.rs` | Agent instance storage & queries | Add `Store::all_workspace_agents()` bulk query |
| `src/app.rs` | App state, refresh, status classification | Add `agent_roster` cache + `live_instances()` helper; convert 2 duplicate call sites |
| `src/ui/dashboard/row.rs` | Single-row composer, column widths, hit spans | `ColumnWidths.agent`, `RowInputs.peers`, strip rendering, `left_consumed`, `pr_chip_hit_span` |
| `src/ui/dashboard/mod.rs` | View dispatch, hit-test walks | Compute derived agent width; thread into hit-test + `render_list` |
| `src/app/render.rs` | Builds `RowInputs` per workspace | Populate `peers` |

---

### Task 1: `Store::all_workspace_agents()` bulk query

**Files:**
- Modify: `src/data/agents.rs` (add method to the `impl Store` block that starts at line 55)
- Test: `src/data/agents.rs` (the existing `#[cfg(test)] mod tests` at the bottom)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `Store::all_workspace_agents(&self) -> Result<std::collections::HashMap<WorkspaceId, Vec<AgentInstance>>>`. Each `Vec` is ordered `is_primary DESC, created_at ASC, id ASC` — identical to the existing per-workspace `workspace_agents()`. Workspaces with no rows are absent from the map.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block at the bottom of `src/data/agents.rs`. Match the existing test style in that file for constructing a repo + workspace.

```rust
    #[test]
    fn all_workspace_agents_groups_by_workspace_and_keeps_primary_first() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "r/")
            .unwrap();
        let mk = |name: &str| {
            store
                .insert_workspace(&crate::data::store::NewWorkspace {
                    repo_id: repo,
                    name,
                    branch: name,
                    worktree_path: std::path::Path::new("/tmp/r/w"),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap()
        };
        let ws_a = mk("a");
        let ws_b = mk("b");
        store.add_primary_agent(ws_a, AgentKind::Claude).unwrap();
        store.add_workspace_agent(ws_a, AgentKind::Codex).unwrap();
        store.add_primary_agent(ws_b, AgentKind::Pi).unwrap();

        let map = store.all_workspace_agents().unwrap();

        // Grouped per workspace.
        assert_eq!(map.get(&ws_a).map(|v| v.len()), Some(2));
        assert_eq!(map.get(&ws_b).map(|v| v.len()), Some(1));
        // Primary first, then creation order — same contract as workspace_agents().
        let a = &map[&ws_a];
        assert!(a[0].is_primary, "primary must sort first");
        assert_eq!(a[0].agent, AgentKind::Claude);
        assert_eq!(a[1].agent, AgentKind::Codex);
        assert!(!a[1].is_primary);
        // Matches the per-workspace query exactly.
        assert_eq!(store.workspace_agents(ws_a).unwrap(), *a);
    }

    #[test]
    fn all_workspace_agents_omits_workspaces_with_no_instances() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "r/")
            .unwrap();
        let ws = store
            .insert_workspace(&crate::data::store::NewWorkspace {
                repo_id: repo,
                name: "lonely",
                branch: "lonely",
                worktree_path: std::path::Path::new("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        assert!(store.all_workspace_agents().unwrap().get(&ws).is_none());
    }
```

The first test compares `AgentInstance` values with `assert_eq!`, so `AgentInstance` needs `PartialEq`. Check its derive list at `src/data/agents.rs:12`; if `PartialEq` is absent, add it:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInstance {
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib data::agents::tests::all_workspace_agents -- --nocapture`
Expected: FAIL — `no method named 'all_workspace_agents' found for struct 'Store'`.

- [ ] **Step 3: Implement the bulk query**

Add to the `impl Store` block in `src/data/agents.rs`, directly after `workspace_agents` (which ends around line 65). This mirrors `all_workspace_status` at `src/data/status.rs:119-134`.

```rust
    /// Every instance in the database, grouped by workspace. Each group is
    /// ordered exactly like `workspace_agents`: primary first, then by
    /// creation time. One statement for the whole table — the dashboard
    /// refreshes this per `App::refresh`, not per frame, so a per-workspace
    /// query in a loop would be needless I/O.
    pub fn all_workspace_agents(
        &self,
    ) -> Result<std::collections::HashMap<WorkspaceId, Vec<AgentInstance>>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, workspace_id, agent, ordinal, is_primary, session_ref, created_at
             FROM workspace_agents
             ORDER BY workspace_id ASC, is_primary DESC, created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], row_to_instance)?;
        let mut map: std::collections::HashMap<WorkspaceId, Vec<AgentInstance>> =
            std::collections::HashMap::new();
        for row in rows {
            let inst = row?;
            map.entry(inst.workspace_id).or_default().push(inst);
        }
        Ok(map)
    }
```

`row_to_instance` is the existing free function above the `impl` block (it ends at `src/data/agents.rs:53`), and `AgentInstance.workspace_id` is already a `WorkspaceId`, so the grouping key needs no conversion.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib data::agents::tests::all_workspace_agents -- --nocapture`
Expected: PASS, both tests.

- [ ] **Step 5: Verify nothing else broke, then commit**

```bash
cargo test --lib data::agents
mise exec rust@1.95.0 -- cargo fmt --all --check
git add src/data/agents.rs
git commit -m "feat(store): add all_workspace_agents bulk query"
```

---

### Task 2: `App::live_instances` helper + roster cache

**Files:**
- Modify: `src/app.rs` — struct field, `refresh()` at line 733, new helper, and two call sites at lines 790-801 and 1982-1994
- Test: `src/app.rs` (existing `#[cfg(test)] mod tests`, or `src/app/input_tests.rs` if the app-construction helpers live there — follow whichever the file already uses for `App`-level tests)

**Interfaces:**
- Consumes: `Store::all_workspace_agents()` from Task 1.
- Produces:
  - `App::agent_roster: std::collections::HashMap<WorkspaceId, Vec<AgentInstance>>` — public field, refilled by `refresh()`.
  - `App::live_instances(&self, ws: WorkspaceId) -> Vec<AgentInstance>` — roster entries whose session is `SessionStatus::Running`, in roster order (primary first). Empty vec for an unknown workspace.

**Why this exists:** the roster-fetch-plus-`Running`-filter idiom is already written out twice (`app.rs:790-801`, `app.rs:1982-1994`). This task collapses both onto one helper and adds the cache that keeps it out of the render hot path.

**Do NOT convert `src/app/render.rs:504`.** It looks similar but has no liveness filter — the attached footer's agent switcher deliberately lists every *registered* instance so exited agents stay switchable and clickable. Converting it would silently drop dead agents out of the switcher.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn live_instances_excludes_exited_and_never_started_peers() {
        let mut app = test_app();
        let ws = app.test_workspace("multi");
        let primary = app.store.add_primary_agent(ws, AgentKind::Claude).unwrap();
        let peer_running = app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let peer_exited = app.store.add_workspace_agent(ws, AgentKind::Pi).unwrap();
        // `peer_never_started` gets no session entry at all.
        let _peer_never_started = app.store.add_workspace_agent(ws, AgentKind::Hermes).unwrap();
        app.refresh().unwrap();

        app.test_spawn_session(primary.id, SessionStatus::Running { pid: 1 });
        app.test_spawn_session(peer_running.id, SessionStatus::Running { pid: 2 });
        app.test_spawn_session(peer_exited.id, SessionStatus::Exited { code: 0 });

        let live: Vec<_> = app.live_instances(ws).into_iter().map(|i| i.id).collect();
        assert_eq!(live, vec![primary.id, peer_running.id]);
    }

    #[test]
    fn live_instances_is_empty_for_unknown_workspace() {
        let app = test_app();
        assert!(app.live_instances(WorkspaceId(9999)).is_empty());
    }

    #[test]
    fn refresh_repopulates_the_agent_roster() {
        let mut app = test_app();
        let ws = app.test_workspace("rostered");
        app.store.add_primary_agent(ws, AgentKind::Claude).unwrap();
        app.refresh().unwrap();
        assert_eq!(app.agent_roster.get(&ws).map(|v| v.len()), Some(1));

        app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        // Stale until refresh — this is the cache contract every mutation
        // path has to respect.
        assert_eq!(app.agent_roster.get(&ws).map(|v| v.len()), Some(1));
        app.refresh().unwrap();
        assert_eq!(app.agent_roster.get(&ws).map(|v| v.len()), Some(2));
    }
```

`test_app()`, `test_workspace()`, and `test_spawn_session()` are placeholders for whatever helpers the existing app tests use. **Before writing these tests, read the existing `#[cfg(test)]` block in `src/app.rs` and `src/app/input_tests.rs` and use the real helper names.** If no helper exists for injecting a session with a chosen `SessionStatus`, add one in the test module rather than spawning a real PTY — `SessionManager.sessions` is private, so the helper needs to live where it can reach `app.sessions`, or `SessionManager` needs a `#[cfg(test)]` insert method.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib live_instances -- --nocapture`
Expected: FAIL — `no method named 'live_instances'` and `no field 'agent_roster'`.

- [ ] **Step 3: Add the field, the refresh hook, and the helper**

Add the field to the `App` struct, next to the other per-workspace maps (`pushed_status`, `workspace_processes`, `pr_lifecycle` — around `app.rs:436-530`):

```rust
    /// Every workspace's agent instances, refilled by `refresh`. Cached so
    /// the per-frame dashboard build can resolve a workspace's agents
    /// without a SQLite round-trip per row. Liveness is NOT cached — it
    /// comes from `sessions`, which is already in memory.
    pub agent_roster: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        Vec<crate::data::agents::AgentInstance>,
    >,
```

Initialize it in whatever constructs `App` (search for the other map initializers, e.g. `pushed_status:`) with `Default::default()`.

In `refresh()` (`app.rs:733`), add the reload next to the existing bulk loads. Put it immediately after the `pushed_status` line so the bulk-load group stays together:

```rust
        self.agent_roster = self.store.all_workspace_agents().unwrap_or_default();
```

Add the helper method to the same `impl App` block:

```rust
    /// The workspace's agent instances that currently have a running
    /// session, in roster order (primary first). Instances registered in
    /// the DB but with no session — never started, or exited — are
    /// excluded: nothing reaps an instance row when its agent exits, so
    /// "registered" and "running" diverge permanently.
    pub fn live_instances(
        &self,
        ws: crate::data::store::WorkspaceId,
    ) -> Vec<crate::data::agents::AgentInstance> {
        self.agent_roster
            .get(&ws)
            .map(|instances| {
                instances
                    .iter()
                    .filter(|inst| {
                        self.sessions.get(inst.id).is_some_and(|s| {
                            matches!(
                                *s.status.read().unwrap(),
                                crate::pty::session::SessionStatus::Running { .. }
                            )
                        })
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib live_instances -- --nocapture && cargo test --lib refresh_repopulates`
Expected: PASS, all three tests.

- [ ] **Step 5: Convert the two duplicate call sites**

At `app.rs:790-801`, replace the inline roster fetch and filter:

```rust
            let has_client = !self.live_instances(ws.id).is_empty();
```

Delete the now-unused `let instances = match self.store.workspace_agents(ws.id) { ... }` block above it. Note this changes error behavior: the old code did `continue` on a store error, the new code treats an error as "no roster entry" — acceptable because `refresh` already swallowed the error with `unwrap_or_default`.

At `app.rs:1982-1994`, replace:

```rust
    let running: Vec<_> = app.live_instances(ws_id);
```

**Ordering hazard:** that call site runs *before* `app.refresh()` on the same code path (`app.rs:1996`), so it reads the roster as of the last refresh. That is correct here — it is enumerating sessions that exist right now, and no instance was added or removed on this path. Do not reorder the refresh.

- [ ] **Step 6: Invalidate the cache on the agents-panel mutation paths**

The agents panel adds and removes instances without going through `refresh()`. Verify and fix both:

- `src/app/input.rs:1879` — the single-add and the `a` key that adds all four kinds (`input.rs:1877-1883`)
- `src/app/input.rs:1893` — the `x` remove key

After each mutation completes, call `app.refresh()?`. Confirm by reading the surrounding handler whether `refresh` is already reached on those paths; if it is, add nothing.

- [ ] **Step 7: Run the full suite and commit**

```bash
cargo test --lib
mise exec rust@1.95.0 -- cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
git add src/app.rs src/app/input.rs
git commit -m "refactor(app): cache the agent roster and extract live_instances"
```

Some `app::input` PTY-timing tests flake under the full suite. If a failure is in that module, re-run it in isolation before treating it as a regression.

---

### Task 3: `ColumnWidths.agent`, `RowInputs.peers`, and strip rendering

**Files:**
- Modify: `src/ui/dashboard/row.rs` — consts (27-39), `ColumnWidths` (41-66), `RowInputs` (68-96), `render` (98-124), `left_consumed` (283-292), `pr_chip_hit_span` (373-378), module doc (5-14)
- Modify (test constructors only): `src/ui/dashboard/by_repo.rs`, `src/ui/dashboard/by_attention.rs`, `src/ui/dashboard/tests.rs`
- Test: `src/ui/dashboard/row.rs` `mod tests`

**Interfaces:**
- Consumes: nothing from earlier tasks — this task is pure rendering and can be built independently.
- Produces:
  - `ColumnWidths { branch: usize, pr: usize, agent: usize }` with `Default.agent == 1` and `ColumnWidths::with_agent(self, n: usize) -> Self` clamping to `1..=MAX_AGENT_WIDTH`.
  - `pub const MAX_AGENT_WIDTH: usize = 4;`
  - `RowInputs.peers: Vec<AgentKind>` — live peers in creation order, primary excluded.
  - `pub fn agent_strip_spans(inputs: &RowInputs, widths: ColumnWidths, theme: &Theme) -> Vec<Span<'static>>` — exactly `widths.agent` chars.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/ui/dashboard/row.rs`. `base()` and `line_text()` are the existing helpers in that module.

```rust
    fn strip_text(inputs: &RowInputs, widths: ColumnWidths) -> String {
        let theme = Theme::wsx();
        agent_strip_spans(inputs, widths, &theme)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn strip_at_width_one_is_a_single_primary_bar() {
        let inputs = base();
        assert_eq!(strip_text(&inputs, ColumnWidths::default()), "▎");
    }

    #[test]
    fn strip_right_aligns_with_primary_last() {
        let mut inputs = base();
        inputs.peers = vec![AgentKind::Codex, AgentKind::Pi];
        // Two peers + primary = 3 bars in a 4-wide field: one pad cell.
        assert_eq!(
            strip_text(&inputs, ColumnWidths::default().with_agent(4)),
            " ▎▎▎"
        );
    }

    #[test]
    fn strip_pads_when_the_row_has_fewer_agents_than_the_column() {
        let inputs = base(); // primary only
        assert_eq!(
            strip_text(&inputs, ColumnWidths::default().with_agent(4)),
            "   ▎"
        );
    }

    #[test]
    fn strip_colors_each_bar_by_its_own_kind_with_primary_rightmost() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Claude;
        inputs.peers = vec![AgentKind::Codex];
        let spans = agent_strip_spans(&inputs, ColumnWidths::default().with_agent(2), &theme);
        let bars: Vec<_> = spans.iter().filter(|s| s.content.contains('▎')).collect();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].style.fg, theme.agent_style(AgentKind::Codex).fg);
        assert_eq!(bars[1].style.fg, theme.agent_style(AgentKind::Claude).fg);
    }

    #[test]
    fn strip_overflows_with_a_plus_marker() {
        let mut inputs = base();
        // 4 peers + primary = 5 live, one more than MAX_AGENT_WIDTH.
        inputs.peers = vec![
            AgentKind::Codex,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Codex,
        ];
        let text = strip_text(&inputs, ColumnWidths::default().with_agent(MAX_AGENT_WIDTH));
        assert_eq!(text, "+▎▎▎");
        assert_eq!(text.chars().count(), MAX_AGENT_WIDTH);
    }

    #[test]
    fn strip_is_always_exactly_the_column_width() {
        for agent_width in 1..=MAX_AGENT_WIDTH {
            for peer_count in 0..6 {
                let mut inputs = base();
                inputs.peers = vec![AgentKind::Codex; peer_count];
                let text = strip_text(&inputs, ColumnWidths::default().with_agent(agent_width));
                assert_eq!(
                    text.chars().count(),
                    agent_width,
                    "width {agent_width}, {peer_count} peers: {text:?}"
                );
            }
        }
    }

    #[test]
    fn with_agent_clamps_to_the_cap() {
        assert_eq!(ColumnWidths::default().with_agent(0).agent, 1);
        assert_eq!(ColumnWidths::default().with_agent(99).agent, MAX_AGENT_WIDTH);
    }

    #[test]
    fn widening_the_strip_shifts_every_later_column_by_the_same_amount() {
        let theme = Theme::wsx();
        let procs_col = |s: &str| s.chars().position(|c| c == '●').unwrap();
        let mut inputs = base();
        inputs.procs = 2;
        let narrow = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        let wide = line_text(&render(
            &inputs,
            ColumnWidths::default().with_agent(3),
            0,
            &theme,
            120,
        ));
        assert_eq!(procs_col(&wide), procs_col(&narrow) + 2);
    }

    #[test]
    fn pr_chip_hit_span_tracks_the_agent_column_width() {
        let mut inputs = base();
        inputs.pr_number = Some(12);
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        let (x_narrow, w_narrow) = pr_chip_hit_span(&inputs, ColumnWidths::default()).unwrap();
        let (x_wide, w_wide) =
            pr_chip_hit_span(&inputs, ColumnWidths::default().with_agent(4)).unwrap();
        assert_eq!(x_wide, x_narrow + 3, "chip must shift with the strip");
        assert_eq!(w_wide, w_narrow, "chip width is unaffected");
    }

    #[test]
    fn hit_span_matches_where_the_chip_actually_renders() {
        // Guards the real failure mode: `pr_chip_hit_span` recomputes the
        // offset independently of `left_consumed`, so the two can silently
        // disagree and send clicks to the wrong column.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.pr_number = Some(12);
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        for agent_width in 1..=MAX_AGENT_WIDTH {
            let widths = ColumnWidths::default().with_agent(agent_width);
            let text = line_text(&render(&inputs, widths, 0, &theme, 160));
            let (x, _) = pr_chip_hit_span(&inputs, widths).unwrap();
            let rendered_at = text.chars().position(|c| c == '⏺').unwrap();
            assert_eq!(
                rendered_at, x as usize,
                "agent_width={agent_width}: hit span disagrees with render"
            );
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::row -- --nocapture`
Expected: FAIL — `no field 'peers'`, `no function 'agent_strip_spans'`, `no method 'with_agent'`, `cannot find value 'MAX_AGENT_WIDTH'`.

- [ ] **Step 3: Widen `ColumnWidths`**

In `src/ui/dashboard/row.rs`, replace `const AGENT_WIDTH: usize = 1;` (line 39) with:

```rust
pub const DEFAULT_AGENT_WIDTH: usize = 1;
/// Cap on the agent strip. Five live agents is one keystroke away — the
/// agents panel's `a` key adds all four kinds at once — so the strip must
/// degrade rather than grow without bound.
pub const MAX_AGENT_WIDTH: usize = 4;
```

Extend the struct and its impls (lines 41-66):

```rust
pub struct ColumnWidths {
    pub branch: usize,
    pub pr: usize,
    /// Derived per frame from the live-agent count across visible rows —
    /// NOT user-configurable and NOT read from settings. Set via
    /// `with_agent` by the view dispatchers in `dashboard::mod`.
    pub agent: usize,
}

impl ColumnWidths {
    pub fn clamped(branch: usize, pr: usize) -> Self {
        Self {
            branch: branch.clamp(MIN_BRANCH_WIDTH, MAX_BRANCH_WIDTH),
            pr: pr.clamp(MIN_PR_WIDTH, MAX_PR_WIDTH),
            agent: DEFAULT_AGENT_WIDTH,
        }
    }

    pub fn with_agent(self, agent: usize) -> Self {
        Self {
            agent: agent.clamp(DEFAULT_AGENT_WIDTH, MAX_AGENT_WIDTH),
            ..self
        }
    }
}

impl Default for ColumnWidths {
    fn default() -> Self {
        Self {
            branch: DEFAULT_BRANCH_WIDTH,
            pr: DEFAULT_PR_WIDTH,
            agent: DEFAULT_AGENT_WIDTH,
        }
    }
}
```

- [ ] **Step 4: Add `RowInputs.peers`**

Add to the struct (after `pub agent: AgentKind,` at line 71):

```rust
    /// Live non-primary agents in this workspace, in creation order. The
    /// primary is `agent` and is rendered unconditionally; peers appear
    /// only while their session is running, so a finished reviewer drops
    /// out and the strip narrows back on its own.
    pub peers: Vec<AgentKind>,
```

This breaks all 9 `RowInputs { .. }` construction sites. Add `peers: Vec::new(),` to each. They are in:
- `src/app/render.rs` (the real one — Task 5 populates it properly)
- `src/ui/dashboard/row.rs` (test `base()`)
- `src/ui/dashboard/by_repo.rs` (tests)
- `src/ui/dashboard/by_attention.rs` (tests)
- `src/ui/dashboard/tests.rs`

Find them all with `grep -rn "RowInputs {" src/`.

- [ ] **Step 5: Implement the strip**

Add the composer as a public function in `row.rs`, above `render`:

```rust
/// The leftmost column: one bar per live agent, right-aligned so the
/// primary stays adjacent to the status gutter and a single-agent row
/// looks exactly as it did before the strip existed. Always returns
/// exactly `widths.agent` chars — the whole row's column alignment
/// depends on it.
pub fn agent_strip_spans(
    inputs: &RowInputs,
    widths: ColumnWidths,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let cells = widths.agent.max(1);
    let total = inputs.peers.len() + 1;
    if total > cells {
        // Overflow: a `+` stands in for the peers that don't fit, then the
        // NEWEST peers, then the primary — the oldest peers are what drop
        // out. With only one cell there's no room for the marker, so the
        // primary alone is the honest render.
        let peer_cells = cells.saturating_sub(2);
        if cells >= 2 {
            spans.push(Span::styled("+".to_string(), theme.dim_style()));
        }
        for kind in &inputs.peers[inputs.peers.len() - peer_cells..] {
            spans.push(Span::styled("▎".to_string(), theme.agent_style(*kind)));
        }
    } else {
        let pad = cells - total;
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        for kind in &inputs.peers {
            spans.push(Span::styled("▎".to_string(), theme.agent_style(*kind)));
        }
    }
    spans.push(Span::styled(
        "▎".to_string(),
        theme.agent_style(inputs.agent),
    ));
    spans
}
```

Char-count check, since `strip_is_always_exactly_the_column_width` sweeps every
combination and will catch an off-by-one immediately:

| cells | peers | branch | output | chars |
|---|---|---|---|---|
| 1 | 0 | pad=0 | `▎` | 1 |
| 4 | 0 | pad=3 | `␣␣␣▎` | 4 |
| 4 | 2 | pad=1 | `␣▎▎▎` | 4 |
| 4 | 4 | overflow, peer_cells=2 | `+▎▎▎` | 4 |
| 2 | 3 | overflow, peer_cells=0 | `+▎` | 2 |
| 1 | 3 | overflow, no marker | `▎` | 1 |
```

Then in `render` (lines 109-124), replace the hard-coded single bar with:

```rust
    // 0: agent identity strip — one fixed-per-kind colored bar per live
    // agent, primary rightmost. Sits left of the status gutter so the row
    // shows a two-tone left edge: outer = agents, inner = status. Plain
    // Unicode, no nerd-font gating (same glyph as the gutter).
    spans.extend(agent_strip_spans(inputs, widths, theme));
```

- [ ] **Step 6: Fix both width consumers**

In `left_consumed` (lines 283-292), replace `AGENT_WIDTH` with `widths.agent`.

In `pr_chip_hit_span` (line 374), replace `AGENT_WIDTH` with `widths.agent`:

```rust
    let x = widths.agent + GUTTER_WIDTH + ELBOW_WIDTH + GLYPH_WIDTH + widths.branch;
```

At this point `AGENT_WIDTH` no longer exists; the compiler will point at any remaining use.

- [ ] **Step 7: Update the stale module doc-comment**

The doc-comment at `row.rs:5-14` omits the agent column entirely. Rewrite the column table to include it as column 0 with its derived width, so the next reader isn't misled the way this design nearly was.

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard -- --nocapture`
Expected: PASS. The pre-existing `unshared_row_has_no_shared_badge_and_widths_stay_aligned` must still pass — it uses `ColumnWidths::default()`, whose `agent` is 1, so today's geometry is unchanged.

- [ ] **Step 9: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
git add src/ui/dashboard/ src/app/render.rs
git commit -m "feat(dashboard): render a multi-agent strip in the row's agent column"
```

---

### Task 4: Derive the strip width in the view dispatchers

**Files:**
- Modify: `src/ui/dashboard/mod.rs` — `render_by_repo` (442-521), `render_by_attention` (524-620)
- Test: `src/ui/dashboard/tests.rs`

**Interfaces:**
- Consumes: `ColumnWidths::with_agent`, `MAX_AGENT_WIDTH`, `RowInputs.peers` from Task 3.
- Produces: `fn derived_agent_width<'a>(rows: impl Iterator<Item = &'a RowInputs>) -> usize` in `src/ui/dashboard/mod.rs` — `clamp(max(1 + peers.len()), 1, MAX_AGENT_WIDTH)` over the given rows; `1` for an empty iterator.

**The point of this task:** `render_by_repo` and `render_by_attention` each use `inputs.column_widths` *twice* — once for the PR-chip hit-test walk (`mod.rs:508`, `mod.rs:608`) and once for `render_list` (`mod.rs:519`, `mod.rs:618`). Both must receive the same widened value. Computing the width here, above both uses, makes that true by construction. Computing it inside `render_list` instead would widen the drawn row while the hit-tester used the old width, offsetting every PR-chip click.

- [ ] **Step 1: Write the failing tests**

Add to `src/ui/dashboard/tests.rs`:

```rust
    #[test]
    fn derived_agent_width_is_one_when_no_workspace_has_peers() {
        let rows = vec![row_with_peers(0), row_with_peers(0)];
        assert_eq!(derived_agent_width(rows.iter()), 1);
    }

    #[test]
    fn derived_agent_width_takes_the_max_across_rows() {
        let rows = vec![row_with_peers(0), row_with_peers(2), row_with_peers(1)];
        assert_eq!(derived_agent_width(rows.iter()), 3);
    }

    #[test]
    fn derived_agent_width_clamps_to_the_cap() {
        let rows = vec![row_with_peers(9)];
        assert_eq!(derived_agent_width(rows.iter()), MAX_AGENT_WIDTH);
    }

    #[test]
    fn derived_agent_width_of_nothing_is_one() {
        let rows: Vec<RowInputs> = Vec::new();
        assert_eq!(derived_agent_width(rows.iter()), 1);
    }

    #[test]
    fn folded_repos_do_not_widen_the_strip() {
        // A peer-heavy workspace inside a collapsed repo is not drawn, so
        // it must not tax the recap column of the rows that ARE drawn.
        let mut inputs = fixture_dashboard_inputs();
        give_workspace_peers(&mut inputs, 0, 3);
        let mut state = DashboardState::default();
        fold_every_repo(&mut state, &inputs);
        let (_items, chips) = render_by_repo(&inputs, &mut state, 0, 160, &Theme::wsx());
        assert!(chips.is_empty(), "folded repos render no rows");
    }

    #[test]
    fn chip_hit_spans_use_the_widened_strip() {
        // The regression this task exists to prevent: hit spans computed at
        // the unwidened width while rows render at the widened one.
        let mut inputs = fixture_dashboard_inputs_with_pr();
        give_workspace_peers(&mut inputs, 0, 2);
        let mut state = DashboardState::default();
        let (items, chips) = render_by_repo(&inputs, &mut state, 0, 160, &Theme::wsx());
        let (_ws, flat_idx, (x, _w)) = chips[0];
        let rendered = item_text(&items[flat_idx]);
        assert_eq!(
            rendered.chars().position(|c| c == '⏺'),
            Some(x as usize),
            "hit span must match where the chip actually rendered:\n  {rendered:?}"
        );
    }
```

`row_with_peers`, `fixture_dashboard_inputs`, `give_workspace_peers`, `fold_every_repo`, and `item_text` are placeholders. **Read `src/ui/dashboard/tests.rs` first** and build on the fixtures already there — it already constructs `DashboardInputs`, and `src/ui/dashboard/fixture.rs` supplies synthetic repos. Add only the helpers that don't exist yet, e.g.:

```rust
    fn row_with_peers(n: usize) -> RowInputs {
        let mut r = base_row(); // whatever the existing helper is named
        r.peers = vec![AgentKind::Codex; n];
        r
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::tests -- --nocapture`
Expected: FAIL — `cannot find function 'derived_agent_width'`.

- [ ] **Step 3: Add the helper**

In `src/ui/dashboard/mod.rs`, above `render_by_repo`:

```rust
/// Cells the agent strip needs for a set of rows: the widest live-agent
/// count among them, capped. Derived per frame from the rows actually
/// being drawn — a peer inside a folded repo or filtered out by the search
/// box must not tax the recap column of the rows that ARE drawn.
fn derived_agent_width<'a>(rows: impl Iterator<Item = &'a RowInputs>) -> usize {
    rows.map(|r| r.peers.len() + 1)
        .max()
        .unwrap_or(1)
        .clamp(1, row::MAX_AGENT_WIDTH)
}
```

- [ ] **Step 4: Use it in `render_by_repo`**

In `render_by_repo`, after `by_repo::order_repos(&mut views);` (line 477) and before the hit-test walk:

```rust
    // Only expanded repos render rows, so only they may widen the strip.
    let widths = inputs.column_widths.with_agent(derived_agent_width(
        views
            .iter()
            .filter(|v| v.expanded)
            .flat_map(|v| v.workspaces.iter()),
    ));
```

Then replace **both** uses of `inputs.column_widths` in this function with `widths`:
- line 508: `row::pr_chip_hit_span(w, widths)`
- line 519: `by_repo::render_list(&views, widths, tick, width, theme)`

- [ ] **Step 5: Use it in `render_by_attention`**

In `render_by_attention`, after `let mut data = by_attention::partition(rows, quiet);` (line 583) and before the hit-test walk:

```rust
    // Quiet repos render no rows, so only the four sections count.
    let widths = inputs.column_widths.with_agent(derived_agent_width(
        [
            &data.needs_attention,
            &data.working,
            &data.recent,
            &data.idle,
        ]
        .into_iter()
        .flat_map(|s| s.iter())
        .map(|r| &r.row),
    ));
```

Then replace **both** uses of `inputs.column_widths` in this function with `widths`:
- line 608: `row::pr_chip_hit_span(&row.row, widths)`
- line 618: `by_attention::render_list(&data, widths, tick, width, theme)`

Grep the file afterwards — `grep -n "inputs.column_widths" src/ui/dashboard/mod.rs` should return nothing inside these two functions.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard -- --nocapture`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
git add src/ui/dashboard/mod.rs src/ui/dashboard/tests.rs
git commit -m "feat(dashboard): derive the agent strip width from visible rows"
```

---

### Task 5: Populate `RowInputs.peers` from live sessions

**Files:**
- Modify: `src/app/render.rs:81-150` (the `RowInputs` construction inside `draw`)
- Test: `src/ui/dashboard/tests.rs` or `src/app/input_tests.rs` — wherever `App`-level render tests already live

**Interfaces:**
- Consumes: `App::live_instances` (Task 2), `RowInputs.peers` (Task 3).
- Produces: nothing new — this is the wiring that makes the feature live.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn row_peers_exclude_the_primary_and_dead_agents() {
        let mut app = test_app();
        let ws = app.test_workspace("multi");
        let primary = app.store.add_primary_agent(ws, AgentKind::Claude).unwrap();
        let live_peer = app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let dead_peer = app.store.add_workspace_agent(ws, AgentKind::Pi).unwrap();
        app.refresh().unwrap();
        app.test_spawn_session(primary.id, SessionStatus::Running { pid: 1 });
        app.test_spawn_session(live_peer.id, SessionStatus::Running { pid: 2 });
        app.test_spawn_session(dead_peer.id, SessionStatus::Exited { code: 0 });

        let peers = app.test_row_inputs(ws).peers;
        assert_eq!(peers, vec![AgentKind::Codex], "primary and dead peer excluded");
    }

    #[test]
    fn row_peers_are_empty_when_the_workspace_was_never_attached() {
        let mut app = test_app();
        let ws = app.test_workspace("cold");
        app.store.add_primary_agent(ws, AgentKind::Claude).unwrap();
        app.refresh().unwrap();
        // No sessions spawned at all.
        assert!(app.test_row_inputs(ws).peers.is_empty());
    }
```

`test_row_inputs` is a placeholder. The `RowInputs` build lives inline inside `draw`, which is awkward to call from a test. **Prefer extracting the per-workspace body of the loop into a named function** — e.g. `fn build_row_inputs(app: &App, ws: &Workspace, now_ms: i64, nerd_fonts: bool) -> RowInputs` — and test that directly. That extraction is part of this task; it also shrinks a `draw` function that is already long.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib row_peers -- --nocapture`
Expected: FAIL — `peers` is always empty (Task 3 initialized it to `Vec::new()` here).

- [ ] **Step 3: Populate `peers`**

In the extracted `build_row_inputs` (or inline at `src/app/render.rs:110`), replace `peers: Vec::new(),` with:

```rust
        // Live peers only, primary excluded — it renders unconditionally as
        // the rightmost bar. Order is roster order (creation time), so a
        // newly added peer lands next to the primary.
        peers: app
            .live_instances(ws.id)
            .into_iter()
            .filter(|inst| !inst.is_primary)
            .map(|inst| inst.agent)
            .collect(),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib row_peers -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Verify by eye**

```bash
cargo build
```

Launch wsx, pick a workspace, press `^x a`, add a codex agent, and confirm:
1. That row's left edge grows a second bar in codex blue, left of the orange primary bar.
2. Every other row gains one leading space, so all columns stay aligned.
3. The recap column loses exactly one character of width.
4. Clicking a PR chip still opens the right PR — this is the hit-test path Task 4 guards.
5. Press `x` in the agents panel to remove the peer; the strip narrows back.

- [ ] **Step 6: Full verification and commit**

```bash
cargo test
mise exec rust@1.95.0 -- cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
git add src/app/render.rs
git commit -m "feat(dashboard): populate row agent peers from live sessions"
```

---

## Self-Review Notes

Spec coverage check against `2026-08-09-multi-agent-dashboard-indicator-design.md`:

| Spec section | Covered by |
|---|---|
| 1. Strip rendering — right-align, primary last, kind colors | Task 3 steps 5, tests in step 1 |
| 1. Overflow `+` marker | Task 3 step 1 (`strip_overflows_with_a_plus_marker`), step 5 |
| 1. Primary never liveness-gated | Task 3 step 5 (primary pushed unconditionally); Task 5 step 1 (`row_peers_are_empty_when_the_workspace_was_never_attached`) |
| 1. Selected-row padding background | **Gap — see below** |
| 2. `ColumnWidths.agent` derived, not from settings | Task 3 step 3; `read_column_widths` deliberately untouched |
| 2. Computed in the dispatchers, shared with hit-test | Task 4 steps 3-5 |
| 3. `left_consumed` + `pr_chip_hit_span` | Task 3 step 6, tests in step 1 |
| 3. Stale module doc-comment | Task 3 step 7 |
| 4. `all_workspace_agents` bulk query | Task 1 |
| 4. `live_instances` + roster cache + 2 call sites | Task 2 |
| 4. `render.rs:504` left alone | Task 2 preamble (explicit warning) |
| 4. Cache invalidation on add/remove | Task 2 step 6 |
| 5. Edge cases | Task 3 step 1 (`strip_is_always_exactly_the_column_width` sweeps 1-4 widths × 0-5 peers); Task 5 step 1 |
| 7. CI gates | Every task's commit step |

**Known gap — selected-row background.** Spec section 1 requires the strip's padding cells to carry the selected-row background so the highlight doesn't gap on the left edge. The plan uses `Span::raw(" ")` for padding, which carries no style. Whether that gaps depends on how the selected row's background is applied — if ratatui paints it on the `ListItem`/`List` highlight style rather than per-span, `Span::raw` is correct and nothing is needed. **Resolve this during Task 3 step 5:** read how `selected` is handled in `row.rs` and `by_repo::render_list`, then either leave `Span::raw` or switch the padding to `Span::styled(" ".repeat(pad), theme.selected_bg_style())` when `inputs.selected`. Add a test asserting the padding's style matches the gutter's background either way.

**Test-helper placeholders.** Tasks 2, 4, and 5 name helpers (`test_app`, `test_workspace`, `test_spawn_session`, `row_with_peers`, `fixture_dashboard_inputs`, `item_text`) that may not exist under those names. Each of those steps says to read the existing test module first and use the real names. `SessionManager.sessions` is private, so injecting a session with a chosen `SessionStatus` may require a `#[cfg(test)]` insert method on `SessionManager` — add it in Task 2 if needed, since that is the first task that requires it.
