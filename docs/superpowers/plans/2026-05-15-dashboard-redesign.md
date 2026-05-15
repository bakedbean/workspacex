# Dashboard Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the workspaces dashboard's loose mixed-density layout with a tabular fixed-column layout: top summary line of state counts, per-repo header + horizontal rule, aligned workspace rows with colored activity words, inline setup-failed glyph, and arrow-nav hint in the footer.

**Architecture:** All changes are scoped to `src/ui/dashboard.rs`. The renderer continues to consume the existing `Item` slice and compute counts/widths internally. New private helpers (`top_summary_line`, `repo_header_lines`, `workspace_main_row`, `workspace_sub_line`, `truncate_pad`, `format_age_compact`, `activity_style`) localize composition logic. The outer `Block::borders(Borders::ALL)` is removed; the selection background remains the visual anchor.

**Tech Stack:** Rust 2024, `ratatui::widgets::{List, ListItem, Paragraph}`, `ratatui::text::{Line, Span}`, `ratatui::style::Style`. No new dependencies.

**Source spec:** `docs/superpowers/specs/2026-05-15-dashboard-redesign-design.md`

**Branch:** `feat/dashboard-redesign` — cut off `main` in Task 1. All work happens here; the user reviews in the running TUI and either merges fast-forward or deletes the branch.

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/ui/dashboard.rs` | Modify | All layout, helpers, and tests |

No other files are touched. Helpers live with their callers per the existing pattern of `format_status`, `format_age`, `format_branch_label`.

---

## Task 1: Cut the feature branch

**Files:** none

- [ ] **Step 1: Confirm working tree is clean and on main**

Run: `git status --short && git rev-parse --abbrev-ref HEAD`
Expected: empty output (clean) and `main`.

- [ ] **Step 2: Cut the branch**

Run: `git checkout -b feat/dashboard-redesign`
Expected: `Switched to a new branch 'feat/dashboard-redesign'`.

- [ ] **Step 3: Verify**

Run: `git status --short && git rev-parse --abbrev-ref HEAD`
Expected: empty output and `feat/dashboard-redesign`.

No commit on this step — the next task's commit is the first on the branch.

---

## Task 2: Top summary line + drop outer border

Replace the static `"wsx — Workspaces"` banner with a dynamic summary computed from the items slice. Remove the `Block::borders(Borders::ALL)` wrapping the list, so workspace rows extend to the full inner width.

**Files:**
- Modify: `src/ui/dashboard.rs`

- [ ] **Step 1: Write the failing tests for the top summary**

Add to `src/ui/dashboard.rs` `mod tests` (place near the other render tests):

```rust
    #[test]
    fn top_summary_shows_total_and_alertable_counts() {
        let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
        let r = repo(1, "demo");
        let w_quiet = workspace(1, 1, "quiet", "wsx/quiet");
        let w_awaiting = workspace(2, 1, "blocked", "wsx/blocked");
        let w_stopped = workspace(3, 1, "thinking", "wsx/thinking");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w_quiet,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w_awaiting,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: true,
                lifecycle: None,
                awaiting_tool: Some(("Bash".into(), 0)),
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w_stopped,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: true,
                lifecycle: None,
                awaiting_tool: None,
                stopped: true,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 12);
        let top = text.lines().next().unwrap().trim();
        assert!(top.contains("wsx"), "missing 'wsx': {top}");
        assert!(top.contains("3 workspaces"), "missing total: {top}");
        assert!(top.contains("1 awaiting"), "missing awaiting count: {top}");
        assert!(top.contains("1 stopped"), "missing stopped count: {top}");
    }

    #[test]
    fn top_summary_omits_zero_alertable_counts() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let top = text.lines().next().unwrap().trim();
        assert!(top.contains("1 workspace"), "missing total: {top}");
        assert!(!top.contains("awaiting"), "unexpected awaiting in quiet top: {top}");
        assert!(!top.contains("stopped"), "unexpected stopped in quiet top: {top}");
    }

    #[test]
    fn outer_border_is_absent() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let buf = term.backend().buffer();
        // No vertical-bar border glyphs should appear at x = 0 anywhere.
        for y in 0..8u16 {
            let cell = buf[(0u16, y)].symbol();
            assert_ne!(cell, "│", "expected no border at col 0, row {y}");
        }
    }
```

- [ ] **Step 2: Run the new tests to confirm they fail**

Run: `cargo test -p wsx --lib ui::dashboard::tests::top_summary_shows_total_and_alertable_counts ui::dashboard::tests::top_summary_omits_zero_alertable_counts ui::dashboard::tests::outer_border_is_absent -- --test-threads=1`
Expected: FAIL — top row is `"wsx — Workspaces"` and border is present.

- [ ] **Step 3: Add the `top_summary_line` helper**

In `src/ui/dashboard.rs`, immediately after `fn format_branch_label`:

```rust
/// Build the top summary line: `wsx · N workspaces[ · K awaiting][ · M stopped]`.
/// State suffixes are omitted when their count is zero. `wsx` uses the header
/// style; ` · `, the numeric totals, and the labels use dim style — except
/// alertable counts (`awaiting`, `stopped`), whose numeric value uses warn.
fn top_summary_line(items: &[Item], theme: &Theme) -> Line<'static> {
    let mut total = 0usize;
    let mut awaiting = 0usize;
    let mut stopped_n = 0usize;
    for item in items {
        if let Item::Workspace {
            awaiting_tool,
            stopped,
            ..
        } = item
        {
            total += 1;
            if awaiting_tool.is_some() {
                awaiting += 1;
            }
            if *stopped {
                stopped_n += 1;
            }
        }
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("wsx".to_string(), theme.header_style()));
    spans.push(Span::styled(
        format!(" · {total} workspace{}", if total == 1 { "" } else { "s" }),
        theme.dim_style(),
    ));
    if awaiting > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{awaiting}"), theme.warn_style()));
        spans.push(Span::styled(" awaiting".to_string(), theme.dim_style()));
    }
    if stopped_n > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{stopped_n}"), theme.warn_style()));
        spans.push(Span::styled(" stopped".to_string(), theme.dim_style()));
    }
    Line::from(spans)
}
```

- [ ] **Step 4: Replace the banner rendering and drop the list border**

In `src/ui/dashboard.rs`, locate the existing `pub fn render(...)`. Two changes to its body:

1. Replace this block (currently around lines 62-63):

```rust
    let header_text = Paragraph::new("wsx — Workspaces").style(theme.header_style());
    f.render_widget(header_text, chunks[0]);
```

with:

```rust
    f.render_widget(
        Paragraph::new(top_summary_line(items, theme)),
        chunks[0],
    );
```

2. Replace the `List::new(list_items)` builder (currently around lines 177-179):

```rust
    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(theme.selected_style());
```

with:

```rust
    let list = List::new(list_items).highlight_style(theme.selected_style());
```

Also remove the now-unused `Borders` and `Block` imports if they become dead — but `Block` may still be referenced by the modal flow; keep them imported if used elsewhere in the file. Check with `cargo build`.

- [ ] **Step 5: Recompute the inner_width without the border**

In the same function, find this line:

```rust
    let inner_width = chunks[1].width.saturating_sub(2) as usize;
```

Replace with:

```rust
    // No outer border anymore — the list spans the full width of chunks[1].
    let inner_width = chunks[1].width as usize;
```

- [ ] **Step 6: Update affected existing tests**

The pre-existing `strip_border_prefix` helper trims `│` and leading spaces. It still works after the border is removed (it'll just be a no-op for the `│`). No change needed.

However, three existing tests assert against the old banner. Find each and remove or revise:

In `pm_state_tests` module (in `src/app.rs`), the test `dashboard_renders_full_area_when_pm_hidden` (search around line 1265) may grep for `"wsx — Workspaces"`. Inspect it and update its substring search to `"wsx · "`.

Run: `grep -n "wsx — Workspaces" src/`
Expected: no matches after fixes. If any matches remain, update each accordingly.

- [ ] **Step 7: Run the new + revised tests**

Run: `cargo test -p wsx --lib ui::dashboard::tests::top_summary_shows_total_and_alertable_counts ui::dashboard::tests::top_summary_omits_zero_alertable_counts ui::dashboard::tests::outer_border_is_absent -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 8: Run the full crate tests**

Run: `cargo test -p wsx -- --test-threads=1`
Expected: all PASS. If any existing dashboard test fails because of the banner change, update its substring assertion the same way.

- [ ] **Step 9: Commit**

```bash
git add src/ui/dashboard.rs src/app.rs
git commit -m "refactor(ui): drop outer dashboard border, replace banner with summary line"
```

---

## Task 3: Repo header with horizontal rule + count suffix

Convert the `Item::Header` rendering from a single `▌ {name}    {path}` line into a two-line block: `{name} · {path} · {count}` plus a horizontal rule of `─` chars spanning the inner width.

**Files:**
- Modify: `src/ui/dashboard.rs`

- [ ] **Step 1: Write failing tests for the new repo-header shape**

Add to `src/ui/dashboard.rs` `mod tests`:

```rust
    #[test]
    fn repo_header_renders_with_rule_below() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let lines: Vec<&str> = text.lines().collect();
        // Find the repo header line; the next non-empty line should be a rule.
        let hdr_idx = lines
            .iter()
            .position(|l| l.contains("demo") && l.contains("/repos/demo"))
            .expect("repo header line");
        let rule = lines[hdr_idx + 1];
        let rule_chars: Vec<char> = rule.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            !rule_chars.is_empty() && rule_chars.iter().all(|c| *c == '─'),
            "expected horizontal rule under header, got: {rule:?}"
        );
    }

    #[test]
    fn repo_header_includes_workspace_count() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w1 = workspace(1, 1, "alpha", "wsx/alpha");
        let w2 = workspace(2, 1, "beta", "wsx/beta");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w1,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w2,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let hdr = text
            .lines()
            .find(|l| l.contains("demo") && l.contains("/repos/demo"))
            .expect("repo header line");
        assert!(hdr.contains("· 2"), "expected workspace count in header: {hdr}");
    }
```

- [ ] **Step 2: Run them to confirm they fail**

Run: `cargo test -p wsx --lib ui::dashboard::tests::repo_header_renders_with_rule_below ui::dashboard::tests::repo_header_includes_workspace_count -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Add the `repo_header_lines` helper**

In `src/ui/dashboard.rs`, add after `top_summary_line`:

```rust
/// Build the two-line block that introduces a repo group:
///   `<name> · <path> · <count>`
///   `─────────────────────────...`
fn repo_header_lines(
    repo: &Repo,
    count: usize,
    inner_width: usize,
    theme: &Theme,
) -> (Line<'static>, Line<'static>) {
    let header = Line::from(vec![
        Span::styled(repo.name.clone(), theme.header_style()),
        Span::styled(
            format!(" · {} · {}", repo.path.display(), count),
            theme.dim_style(),
        ),
    ]);
    let rule_text: String = "─".repeat(inner_width);
    let rule = Line::from(Span::styled(rule_text, theme.dim_style()));
    (header, rule)
}
```

- [ ] **Step 4: Wire `repo_header_lines` into the render loop**

In `src/ui/dashboard.rs`, modify `pub fn render(...)`. First, BEFORE the existing `for item in items.iter()` loop, compute a per-repo workspace count map by scanning the items in one pass:

```rust
    // Count workspaces between each Item::Header. We can't simply count
    // by repo.id during the render loop because we need the count BEFORE
    // emitting the header line.
    let mut counts_by_repo_idx: Vec<usize> = Vec::new();
    {
        let mut current: Option<usize> = None;
        for item in items.iter() {
            match item {
                Item::Header { .. } => {
                    counts_by_repo_idx.push(0);
                    current = Some(counts_by_repo_idx.len() - 1);
                }
                Item::Workspace { .. } => {
                    if let Some(i) = current {
                        counts_by_repo_idx[i] += 1;
                    }
                }
                _ => {}
            }
        }
    }
    let mut repo_idx = 0usize;
```

Then locate the `Item::Header { repo }` match arm. Currently it pushes ONE list item:

```rust
            Item::Header { repo } => {
                if let Some(SelectionTarget::Repo(id)) = selected
                    && id == repo.id
                {
                    selected_idx = Some(list_items.len());
                }
                let line = format!("▌ {}    {}", repo.name, repo.path.display());
                list_items.push(ListItem::new(line).style(theme.header_style()));
            }
```

Replace with:

```rust
            Item::Header { repo } => {
                if let Some(SelectionTarget::Repo(id)) = selected
                    && id == repo.id
                {
                    selected_idx = Some(list_items.len());
                }
                let count = counts_by_repo_idx.get(repo_idx).copied().unwrap_or(0);
                repo_idx += 1;
                let (header, rule) = repo_header_lines(repo, count, inner_width, theme);
                list_items.push(ListItem::new(header));
                list_items.push(ListItem::new(rule));
            }
```

- [ ] **Step 5: Update existing tests that assert the old header shape**

Pre-existing tests in `dashboard.rs` that assert `▌ demo` or similar:

- `renders_repo_header_with_indented_workspace` (line ~337): change
  ```rust
  assert!(text.contains("▌ demo"), "missing header: {text}");
  ```
  to:
  ```rust
  assert!(text.contains("demo") && text.contains("/repos/demo"), "missing header: {text}");
  ```

- `renders_multiple_repos_grouped` (line ~390): similarly update both repo
  header assertions from `▌ first` / `▌ second` style to substring checks
  on `first` / `second`.

Search for the old glyph to catch any remaining sites:

Run: `grep -n '▌' src/`
Expected: no matches after updates.

- [ ] **Step 6: Run tests**

Run: `cargo test -p wsx --lib ui::dashboard:: -- --test-threads=1`
Expected: all dashboard tests PASS, including the two new ones.

- [ ] **Step 7: Run full suite**

Run: `cargo test -p wsx -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add src/ui/dashboard.rs
git commit -m "refactor(ui): repo header with horizontal rule + count suffix"
```

---

## Task 4: Fixed-column workspace row + colored activity word

The main visual change. Rewrite the workspace main-row composition to use fixed-width columns so names, branches, and the right-side activity/age line up vertically across rows. Color the activity word per state.

**Files:**
- Modify: `src/ui/dashboard.rs`

- [ ] **Step 1: Add column constants + helpers**

In `src/ui/dashboard.rs`, near the top of the file (just after the existing imports), add:

```rust
// Column widths for the workspace row. Names and branches are truncated
// or right-padded so the columns align vertically across rows.
const NAME_WIDTH: usize = 20;
const BRANCH_BLOCK_WIDTH: usize = 28;
```

Add the following private helpers below `format_branch_label`:

```rust
/// Right-pad `s` with spaces to `target` chars. If `s` is longer, truncate
/// to `target - 1` chars and append `…`. char-count based (handles
/// multi-byte chars correctly for the alignment math we care about).
fn truncate_pad(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len == target {
        s.to_string()
    } else if len < target {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(target - len));
        out
    } else {
        let mut out: String = s.chars().take(target.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Compact relative-time label for the right-side age column: `5s`, `12s`,
/// `5m`, `1h`. Returns `—` (em-dash) when timestamp is 0 (sentinel for "no
/// meaningful age").
fn format_age_compact(timestamp_ms: i64) -> String {
    if timestamp_ms <= 0 {
        return "—".to_string();
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let secs = ((now_ms - timestamp_ms) / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

/// Map an activity word to a style (color) per the spec.
fn activity_style(label: &str, theme: &Theme) -> Style {
    match label {
        "awaiting" | "stopped" => theme.warn_style(),
        "active" => theme.ok_style(),
        "waiting" | "resumable" | "off" => theme.dim_style(),
        _ => Style::default(),
    }
}
```

You will likely need `use ratatui::style::Style;` added at the top — check the existing imports and add only if missing.

- [ ] **Step 2: Add the new `workspace_main_row` helper**

Add below `activity_style`:

```rust
/// Compose a workspace's main row as a `Line` of spans with fixed columns.
/// Right-justifies the activity + age at the inner-width edge.
#[allow(clippy::too_many_arguments)]
fn workspace_main_row(
    workspace: &Workspace,
    session_running: bool,
    seconds_since_activity: Option<u64>,
    has_prior_session: bool,
    status: Option<crate::git::WorkspaceStatus>,
    needs_attention: bool,
    lifecycle: Option<crate::forge::BranchLifecycle>,
    awaiting_tool: &Option<(String, i64)>,
    stopped: bool,
    nerd: bool,
    theme: &Theme,
    inner_width: usize,
) -> Line<'static> {
    let dot = match (session_running, &workspace.state, has_prior_session) {
        (true, _, _) => "●",
        (false, WorkspaceState::Failed, _) => "✕",
        (false, _, true) => "↻",
        _ => "○",
    };
    let activity = if awaiting_tool.is_some() {
        "awaiting"
    } else if stopped {
        "stopped"
    } else {
        match (seconds_since_activity, has_prior_session) {
            (Some(s), _) if s < 2 => "active",
            (Some(s), _) if s < 30 => "idle",
            (Some(_), _) => "waiting",
            (None, true) => "resumable",
            (None, false) => "off",
        }
    };
    // Age source: the most recent of awaiting_tool.first_seen_ms and
    // (implicit) latest event isn't available here, so we use 0 as a
    // sentinel — the sub-line carries the latest event's age.
    let age_ms = match awaiting_tool {
        Some((_, ts)) => *ts,
        None => 0,
    };
    let name_padded = truncate_pad(&workspace.name, NAME_WIDTH);
    let branch_line = format_branch_label(&workspace.branch, nerd, lifecycle, theme);
    // Take the styled spans from branch_line; pad/truncate to BRANCH_BLOCK_WIDTH.
    let branch_concat: String = branch_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let branch_style = branch_line
        .spans
        .iter()
        .find_map(|s| s.style.fg)
        .map(|fg| Style::default().fg(fg));
    let branch_padded = truncate_pad(&branch_concat, BRANCH_BLOCK_WIDTH);
    let git_status = status
        .map(|s| format_status(&s, nerd))
        .unwrap_or_default();
    let age = format_age_compact(age_ms);

    let attn = if needs_attention { "!" } else { " " };

    // Left side: indent + attn + glyph + name + gutter + branch + gutter + git
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(attn.to_string(), theme.warn_style()));
    spans.push(Span::raw(format!(" {dot} ")));
    spans.push(Span::raw(name_padded));
    spans.push(Span::raw("   ".to_string()));
    match branch_style {
        Some(style) => spans.push(Span::styled(branch_padded, style)),
        None => spans.push(Span::raw(branch_padded)),
    }
    spans.push(Span::raw("   ".to_string()));
    if !git_status.is_empty() {
        spans.push(Span::styled(git_status.clone(), theme.dim_style()));
    }

    // Right side: activity + space + age
    let right_text_w = activity.chars().count() + 1 + age.chars().count();
    let left_w: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = inner_width
        .saturating_sub(left_w + right_text_w)
        .max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(activity.to_string(), activity_style(activity, theme)));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(age, theme.dim_style()));
    Line::from(spans)
}
```

- [ ] **Step 3: Replace the workspace match arm to use the new helper**

In `src/ui/dashboard.rs`, locate the existing `Item::Workspace { ... } => { ... }` arm in `render()`. Replace the entire arm with:

```rust
            Item::Workspace {
                repo: _,
                workspace,
                session_running,
                seconds_since_activity,
                has_prior_session,
                status,
                latest_event,
                needs_attention,
                lifecycle,
                awaiting_tool,
                stopped,
            } => {
                if let Some(SelectionTarget::Workspace(id)) = selected
                    && id == workspace.id
                {
                    selected_idx = Some(list_items.len());
                }
                let main = workspace_main_row(
                    workspace,
                    *session_running,
                    *seconds_since_activity,
                    *has_prior_session,
                    *status,
                    *needs_attention,
                    *lifecycle,
                    awaiting_tool,
                    *stopped,
                    nerd_fonts,
                    theme,
                    inner_width,
                );
                list_items.push(ListItem::new(main));
                // Sub-line: if awaiting, render the permission prompt;
                // otherwise fall back to latest event. Setup-failed glyph
                // lives in the main row's name column in Task 5.
                if let Some((tool_name, first_seen_ms)) = awaiting_tool {
                    let age = format_age(*first_seen_ms);
                    let sub = format!(
                        "      └ ⚠ awaiting permission: {} ({})",
                        tool_name, age
                    );
                    list_items.push(ListItem::new(sub).style(theme.dim_style()));
                } else if let Some(ev) = latest_event {
                    let age = format_age(ev.timestamp_ms);
                    let sub = format!("      └ {} ({})", ev.display, age);
                    list_items.push(ListItem::new(sub).style(theme.dim_style()));
                }
            }
```

Note the sub-line indent changed from 4 to 6 spaces to match the new name column start (indent 2 + attn 1 + sep 1 + glyph 1 + sep 1 = 6).

- [ ] **Step 4: Add tests for column alignment + activity coloring**

Add to `mod tests`:

```rust
    #[test]
    fn workspace_row_name_padded_to_fixed_width() {
        let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
        let r = repo(1, "demo");
        let w_short = workspace(1, 1, "ab", "wsx/ab");
        let w_long = workspace(2, 1, "much-longer-name", "wsx/much-longer-name");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w_short,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w_long,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let buf = term.backend().buffer();
        // Find the y of each workspace row by scanning the buffer for the
        // name. Then check that the glyph after the name column starts at
        // the same x on both rows.
        let find_y = |needle: &str| -> u16 {
            for y in 0..12u16 {
                let row: String = (0..120u16).map(|x| buf[(x, y)].symbol().to_string()).collect();
                if row.contains(needle) {
                    return y;
                }
            }
            panic!("not found: {needle}");
        };
        let y_short = find_y("ab ");
        let y_long = find_y("much-longer-name");
        // Branch column should start at the same x on both rows.
        // x = 2 (indent) + 1 (attn) + 1 (sep) + 1 (dot) + 1 (sep) + 20 (name) + 3 (gutter) = 29
        let probe_x = 29u16;
        // After truncation/padding, both rows' branch glyph should appear at
        // probe_x — the branch glyph differs but its starting x should match.
        let short_at = buf[(probe_x, y_short)].symbol();
        let long_at = buf[(probe_x, y_long)].symbol();
        // Both should be non-space (the branch glyph or first branch char).
        assert!(
            short_at != " " && long_at != " ",
            "branch column misaligned: short={short_at:?} long={long_at:?}"
        );
    }

    #[test]
    fn workspace_row_branch_truncated_with_ellipsis() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let very_long_branch = "feat/the-quick-brown-fox-jumps-over-the-lazy-dog";
        let w = workspace(1, 1, "alpha", very_long_branch);
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let row = text
            .lines()
            .find(|l| l.contains("alpha"))
            .expect("alpha row");
        assert!(
            row.contains('…'),
            "expected branch ellipsis truncation: {row}"
        );
    }

    #[test]
    fn activity_word_uses_warn_color_for_stopped() {
        // Direct unit test of the style mapping.
        let theme = Theme::default_theme();
        let style_stopped = activity_style("stopped", &theme);
        let style_awaiting = activity_style("awaiting", &theme);
        assert_eq!(style_stopped.fg, Some(theme.warn));
        assert_eq!(style_awaiting.fg, Some(theme.warn));
    }

    #[test]
    fn activity_word_uses_ok_color_for_active() {
        let theme = Theme::default_theme();
        let style = activity_style("active", &theme);
        assert_eq!(style.fg, Some(theme.ok));
    }

    #[test]
    fn activity_word_uses_dim_for_off_and_resumable() {
        let theme = Theme::default_theme();
        assert_eq!(activity_style("off", &theme).fg, Some(theme.dim));
        assert_eq!(activity_style("resumable", &theme).fg, Some(theme.dim));
    }

    #[test]
    fn sub_line_indent_aligns_with_name_column() {
        use crate::events::{EventKind, EventSnapshot};
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let ev = EventSnapshot {
            kind: EventKind::AssistantText,
            display: "hello".into(),
            timestamp_ms: now - 5_000,
        };
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: Some(ev),
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let buf = term.backend().buffer();
        // Find the sub-line row (contains "hello") and confirm the └ glyph
        // is at column 6.
        let mut sub_y = None;
        for y in 0..8u16 {
            let row: String = (0..120u16).map(|x| buf[(x, y)].symbol().to_string()).collect();
            if row.contains("hello") && row.contains('└') {
                sub_y = Some(y);
                break;
            }
        }
        let y = sub_y.expect("sub-line not found");
        assert_eq!(buf[(6u16, y)].symbol(), "└");
    }
```

- [ ] **Step 5: Update existing tests that reference the old row shape**

Several existing tests in `dashboard.rs` `mod tests` assert specifics about the old row layout. Inspect each in turn and update string assertions to substring checks that survive the new layout:

- `activity_is_right_justified` — the activity word is still right-justified; the test should still pass but inspect its assertions. If it uses exact x positions, update them based on the new column math.
- `renders_event_subline_when_event_present` — sub-line indent changed from 4 to 6. Update if it asserts on the indent count.
- `renders_awaiting_overrides_activity_and_sub_line` — sub-line indent same change.

Run the tests to discover failures, then fix each in-place.

Run: `cargo test -p wsx --lib ui::dashboard:: -- --test-threads=1 2>&1 | tail -40`

For each failing test, read its body, fix the assertion, and re-run.

- [ ] **Step 6: Full suite green**

Run: `cargo test -p wsx -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 7: Clippy + fmt clean**

Run: `cargo fmt && cargo clippy -p wsx --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/ui/dashboard.rs
git commit -m "feat(ui): fixed-column workspace row with colored activity word"
```

---

## Task 5: Inline setup-failed glyph beside name

Move the `[setup-failed]` indicator out of the right-side region and into the name column as a small `⚙!` glyph in `theme.err_style()`. Truncate the name when needed so the badge fits within the 20-char name allotment.

**Files:**
- Modify: `src/ui/dashboard.rs`

- [ ] **Step 1: Write failing test**

Add to `mod tests`:

```rust
    #[test]
    fn setup_failed_glyph_appears_after_name() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let mut w = workspace(1, 1, "alpha", "wsx/alpha");
        w.setup_status = SetupStatus::Failed;
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let row = text.lines().find(|l| l.contains("alpha")).expect("row");
        assert!(
            row.contains("⚙!"),
            "expected ⚙! setup-failed glyph after name: {row}"
        );
        assert!(
            !row.contains("[setup-failed]"),
            "did not expect the old right-side badge: {row}"
        );
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p wsx --lib ui::dashboard::tests::setup_failed_glyph_appears_after_name -- --test-threads=1`
Expected: FAIL — workspace row has no setup-failed indicator at all yet (Task 4 dropped the right-side badge implicitly because `workspace_main_row` doesn't emit one).

- [ ] **Step 3: Modify `workspace_main_row` to render the inline glyph**

In `src/ui/dashboard.rs`, in `workspace_main_row`, replace:

```rust
    let name_padded = truncate_pad(&workspace.name, NAME_WIDTH);
```

with:

```rust
    // When setup failed, reserve 3 chars (" ⚙!") at the end of the name
    // column and truncate the name to 17 chars so the total stays at 20.
    let setup_failed = workspace.setup_status == SetupStatus::Failed;
    let name_padded = if setup_failed {
        let trimmed = truncate_pad(&workspace.name, NAME_WIDTH - 3);
        // No styled span here yet — we emit the badge as a separate styled
        // span below so it gets err coloring.
        trimmed
    } else {
        truncate_pad(&workspace.name, NAME_WIDTH)
    };
```

Then, in the same function, replace:

```rust
    spans.push(Span::raw(name_padded));
```

with:

```rust
    if setup_failed {
        spans.push(Span::raw(name_padded));
        spans.push(Span::styled(" ⚙!".to_string(), theme.err_style()));
    } else {
        spans.push(Span::raw(name_padded));
    }
```

- [ ] **Step 4: Run the new test**

Run: `cargo test -p wsx --lib ui::dashboard::tests::setup_failed_glyph_appears_after_name -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Run full suite**

Run: `cargo test -p wsx -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard.rs
git commit -m "feat(ui): inline setup-failed glyph beside name"
```

---

## Task 6: Footer arrow-nav hint

Add `[↑/↓] move  ` prefix to the footer so navigation is discoverable.

**Files:**
- Modify: `src/ui/dashboard.rs`

- [ ] **Step 1: Write failing test**

Add to `mod tests`:

```rust
    #[test]
    fn footer_includes_arrow_nav_hint() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let footer = text.lines().last().unwrap();
        assert!(
            footer.contains("[↑/↓] move"),
            "footer missing arrow nav hint: {footer}"
        );
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p wsx --lib ui::dashboard::tests::footer_includes_arrow_nav_hint -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Update footer text**

In `src/ui/dashboard.rs`, find the footer rendering inside `render()`:

```rust
    let footer = Paragraph::new(
        "[enter] attach   [n] new   [e] edit   [t] terminal   [d] archive   [q] quit",
    )
    .style(theme.dim_style());
    f.render_widget(footer, chunks[2]);
```

Replace the literal string with:

```rust
    let footer = Paragraph::new(
        "[↑/↓] move   [enter] attach   [n] new   [e] edit   [t] terminal   [d] archive   [q] quit",
    )
    .style(theme.dim_style());
    f.render_widget(footer, chunks[2]);
```

- [ ] **Step 4: Run the new test**

Run: `cargo test -p wsx --lib ui::dashboard::tests::footer_includes_arrow_nav_hint -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Full suite + clippy**

Run: `cargo test -p wsx -- --test-threads=1`
Expected: all PASS.

Run: `cargo fmt --check && cargo clippy -p wsx --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard.rs
git commit -m "chore(ui): footer hint includes [↑/↓] move"
```

---

## Final verification (no commit)

- [ ] **Step 1: Open the TUI and eyeball the result**

```bash
cargo run -- 2>/dev/null
```

Walk through:
- Top row reads `wsx · N workspaces[ · K awaiting][ · M stopped]`.
- Each repo group: name in header color, dim path + count, faint rule below.
- Workspace rows: names line up vertically; branches line up vertically; activity word colored per state.
- Setup-failed workspaces show inline `⚙!` after the name.
- Sub-line `└` glyph sits at column 6 (right under the name column start).
- Footer shows `[↑/↓] move` first.

- [ ] **Step 2: Report back to user for branch disposition**

The user reviews the running TUI and decides:
- **Merge:** `git checkout main && git merge --ff-only feat/dashboard-redesign && git push origin main`
- **Discard:** `git checkout main && git branch -D feat/dashboard-redesign`

Do not execute either without explicit user instruction.

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| Top summary line | Task 2 |
| Drop outer border | Task 2 |
| Repo header + horizontal rule + count | Task 3 |
| Workspace row fixed columns | Task 4 |
| Activity color map | Task 4 (helper + tests) |
| Compact age column | Task 4 (`format_age_compact`) |
| Sub-line indent at column 6 | Task 4 (sub-line literal indent change) |
| Inline setup-failed glyph | Task 5 |
| Footer arrow-nav hint | Task 6 |
| Branch isolation | Task 1 |

No gaps.

**Placeholder scan:** Every step contains exact code or an exact command. No "TBD"/"implement later"/"similar to Task N". The "for each failing test, fix in-place" instruction in Task 4 Step 5 is intentionally exploratory because we can't know which assertions break without running; this is the only step that requires judgment, and the engineer has the surrounding context to make it.

**Type consistency:** `workspace_main_row` signature uses `Option<crate::git::WorkspaceStatus>` (copied via the existing `status.copied()` upstream pattern), `Option<crate::forge::BranchLifecycle>` (copied via `lifecycle.copied()`), `&Option<(String, i64)>` for `awaiting_tool` (because we need both String and i64 inside). `truncate_pad`, `format_age_compact`, `activity_style` match their call sites. `NAME_WIDTH = 20`, `BRANCH_BLOCK_WIDTH = 28` used consistently. Sub-line indent of 6 spaces matches name column start (indent 2 + attn 1 + sep 1 + glyph 1 + sep 1 = 6).
