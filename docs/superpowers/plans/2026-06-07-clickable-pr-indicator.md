# Clickable PR Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing PR lifecycle chip in the dashboard detail-bar header show the PR number and open the PR in the browser when clicked.

**Architecture:** Extend the existing `gh pr view` poll to also capture the PR number, store it in a parallel per-workspace map, render it inside the existing lifecycle chip, track the chip's screen rect, and hit-test mouse clicks against it to shell out to `gh pr view <branch> --web`. Reuses the established "draw-populates-rect / input-reads-rect" click pattern (`chip_rects`, `attention_rects`, `agent_chip_rects`).

**Tech Stack:** Rust, ratatui, tokio, `gh` CLI, serde.

**Spec:** `docs/superpowers/specs/2026-06-07-clickable-pr-indicator-design.md`

---

## File Structure

- `src/git/forge.rs` — PR fetch/parse (`PrStatus`, `fetch_pr_status`, `number` field); new `open_pr_in_browser` + `pr_web_argv` helpers; tests.
- `src/app.rs` — new `pr_number` map and `pr_link_rect` field + their init.
- `src/app/render.rs` — clear `pr_link_rect` each frame; feed `pr_number` into detail inputs; store the returned chip rect.
- `src/app/background.rs` — store the PR number on poll; clear it on branch rename.
- `src/ui/dashboard/detail.rs` — `build_header_strip` renders `#<n>` and returns the chip rect; `DetailDrawOutput.pr_link_rect`; tests.
- `src/app/input.rs` — mouse hit-test branch + `open_pr_for_workspace`.

Note: `DetailInputs` and `DetailContext` already carry an (unused) `pr_number: Option<u32>` field — we reuse it rather than adding a new one.

---

## Task 1: Fetch & parse the PR number in `forge.rs`

**Files:**
- Modify: `src/git/forge.rs`
- Modify: `src/app/background.rs:348-353` (sole caller of the renamed fn)

- [ ] **Step 1: Update the existing parse tests to the new shape and add number tests**

In `src/git/forge.rs`, the test module currently calls `parse_gh_pr_view(json)` and compares to `Some(BranchLifecycle::X)`. Replace each existing parse assertion to go through the new `parse_gh_pr_status` and compare `.map(|s| s.lifecycle)`, and add number coverage. Replace the six parse tests (`parses_open_pr`, `parses_open_pr_when_mergeable_missing`, `parses_draft_pr`, `parses_conflicted_pr`, `conflict_overrides_draft`, `parses_merged_pr`, `parses_closed_pr`, `parser_returns_none_for_garbage`) and add two new ones:

```rust
    #[test]
    fn parses_open_pr() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":7}"#;
        let s = parse_gh_pr_status(json).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrOpen);
        assert_eq!(s.number, Some(7));
    }

    #[test]
    fn parses_open_pr_when_mergeable_missing() {
        let json = r#"{"state":"OPEN","isDraft":false,"number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrOpen)
        );
    }

    #[test]
    fn parses_draft_pr() {
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"MERGEABLE","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrDraft)
        );
    }

    #[test]
    fn parses_conflicted_pr() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"CONFLICTING","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrConflicted)
        );
    }

    #[test]
    fn conflict_overrides_draft() {
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"CONFLICTING","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrConflicted)
        );
    }

    #[test]
    fn parses_merged_pr() {
        let json = r#"{"state":"MERGED","isDraft":false,"mergeable":"UNKNOWN","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrMerged)
        );
    }

    #[test]
    fn parses_closed_pr() {
        let json = r#"{"state":"CLOSED","isDraft":false,"mergeable":"UNKNOWN","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrClosed)
        );
    }

    #[test]
    fn parser_returns_none_for_garbage() {
        assert!(parse_gh_pr_status("not json").is_none());
        assert!(parse_gh_pr_status("").is_none());
        assert!(parse_gh_pr_status(r#"{"state":"WAT"}"#).is_none());
    }

    #[test]
    fn parses_pr_number() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":152}"#;
        assert_eq!(parse_gh_pr_status(json).unwrap().number, Some(152));
    }

    #[test]
    fn tolerates_missing_number() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE"}"#;
        let s = parse_gh_pr_status(json).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrOpen);
        assert_eq!(s.number, None);
    }
```

Also update the non-git-path test to the renamed fn:

```rust
    #[tokio::test]
    async fn fetch_returns_none_on_non_git_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = fetch_pr_status(tmp.path(), "main").await;
        assert!(matches!(result, Ok(None)), "got {result:?}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p wsx --lib git::forge 2>&1 | tail -20`
Expected: FAIL — `parse_gh_pr_status` / `fetch_pr_status` / `PrStatus.number` not found.

- [ ] **Step 3: Add the `number` field, `PrStatus`, `parse_gh_pr_status`, and rename the fetch fn**

In `src/git/forge.rs`, add `number` to `GhPrView`:

```rust
#[derive(Debug, Deserialize)]
struct GhPrView {
    state: String,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(default)]
    number: Option<u32>,
}
```

Add the status struct after `BranchLifecycle`:

```rust
/// A branch's PR status: its lifecycle plus the PR number (when known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrStatus {
    pub lifecycle: BranchLifecycle,
    pub number: Option<u32>,
}
```

Replace `parse_gh_pr_view` with `parse_gh_pr_status` (same lifecycle logic, now carrying the number):

```rust
/// Parse the JSON returned by
/// `gh pr view <branch> --json state,isDraft,mergeable,number`.
/// Returns the PR status for a known PR, or `None` if the JSON is missing
/// or unparseable (callers treat unknown as "no info").
///
/// Priority for open PRs: CONFLICTING wins over draft, because a conflict
/// requires action regardless of whether the PR is marked ready.
pub(crate) fn parse_gh_pr_status(stdout: &str) -> Option<PrStatus> {
    let parsed: GhPrView = serde_json::from_str(stdout.trim()).ok()?;
    let conflicted = parsed.mergeable.as_deref() == Some("CONFLICTING");
    let lifecycle = match parsed.state.as_str() {
        "OPEN" if conflicted => BranchLifecycle::PrConflicted,
        "OPEN" if parsed.is_draft => BranchLifecycle::PrDraft,
        "OPEN" => BranchLifecycle::PrOpen,
        "MERGED" => BranchLifecycle::PrMerged,
        "CLOSED" => BranchLifecycle::PrClosed,
        _ => return None,
    };
    Some(PrStatus {
        lifecycle,
        number: parsed.number,
    })
}
```

Rename `fetch_branch_lifecycle` → `fetch_pr_status`, returning `Result<Option<PrStatus>>`. The `--json` arg list gains `number`; the `NoPr` path yields `number: None`:

```rust
pub async fn fetch_pr_status(worktree: &Path, branch: &str) -> Result<Option<PrStatus>> {
    let out = Command::new("gh")
        .current_dir(worktree)
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "state,isDraft,mergeable,number",
        ])
        .output()
        .await;

    let out = match out {
        Ok(o) => o,
        // gh not installed, not on PATH, permission error, etc. — degrade.
        Err(_) => return Ok(None),
    };

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Ok(parse_gh_pr_status(&stdout));
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr_means_no_pr(&stderr) {
        return Ok(Some(PrStatus {
            lifecycle: BranchLifecycle::NoPr,
            number: None,
        }));
    }

    // Auth failure, non-GitHub remote, network blip — degrade.
    Ok(None)
}
```

- [ ] **Step 4: Update the sole caller in `background.rs`**

In `src/app/background.rs`, the poll block currently reads:

```rust
                if let Ok(Some(lifecycle)) =
                    crate::git::forge::fetch_branch_lifecycle(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, lifecycle);
                }
```

Replace with (number storage is wired in Task 3; for now just the lifecycle, via the new fn):

```rust
                if let Ok(Some(status)) =
                    crate::git::forge::fetch_pr_status(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, status.lifecycle);
                }
```

- [ ] **Step 5: Run the forge tests to verify they pass**

Run: `cargo test -p wsx --lib git::forge 2>&1 | tail -20`
Expected: PASS (all parse tests including `parses_pr_number`, `tolerates_missing_number`).

- [ ] **Step 6: Build to confirm the rename has no stragglers**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: builds (warnings OK).

- [ ] **Step 7: Commit**

```bash
git add src/git/forge.rs src/app/background.rs
git commit -m "feat(forge): capture PR number in fetch_pr_status"
```

---

## Task 2: Add `pr_number` map and `pr_link_rect` field to `App`

**Files:**
- Modify: `src/app.rs:147-150` (field decls) and `src/app.rs:333` (init)
- Modify: `src/app/render.rs:34` area (per-frame clear)

- [ ] **Step 1: Add the field declarations**

In `src/app.rs`, just after the `pr_lifecycle` field (ends at line 150), add:

```rust
    /// Cached PR number per workspace, populated alongside `pr_lifecycle`.
    /// Absent key = unknown. Used to render `#<n>` in the detail-bar chip.
    pub pr_number: std::collections::HashMap<crate::data::store::WorkspaceId, u32>,
    /// Screen rect of the clickable PR chip in the detail-bar header, with
    /// the workspace it belongs to. Set during draw, read by the mouse
    /// handler. Mirrors the `chip_rects` draw-populates / input-reads pattern.
    pub pr_link_rect: Option<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
```

- [ ] **Step 2: Initialize the fields**

In `src/app.rs`, just after `pr_lifecycle: std::collections::HashMap::new(),` (line 333), add:

```rust
            pr_number: std::collections::HashMap::new(),
            pr_link_rect: None,
```

- [ ] **Step 3: Clear `pr_link_rect` at the start of every frame**

In `src/app/render.rs`, in the clear block (after line 34, `app.agent_chip_rects.clear();`), add:

```rust
    app.pr_link_rect = None;
```

- [ ] **Step 4: Build to verify**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: builds (the new fields are `pub`, so no dead-code warnings).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/app/render.rs
git commit -m "feat(app): add pr_number cache and pr_link_rect field"
```

---

## Task 3: Store the PR number on poll; clear it on branch rename

**Files:**
- Modify: `src/app/background.rs:348-353` (poll store) and `src/app/background.rs:278` (rename clear)

- [ ] **Step 1: Store the number alongside the lifecycle on a successful poll**

In `src/app/background.rs`, update the poll block from Task 1 Step 4 to also write `pr_number`:

```rust
                if let Ok(Some(status)) =
                    crate::git::forge::fetch_pr_status(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, status.lifecycle);
                    match status.number {
                        Some(n) => {
                            g.pr_number.insert(id, n);
                        }
                        None => {
                            g.pr_number.remove(&id);
                        }
                    }
                }
```

- [ ] **Step 2: Clear the number when the branch is renamed**

In `src/app/background.rs`, next to `g.pr_lifecycle.remove(&id);` (line 278), add:

```rust
                    g.pr_number.remove(&id);
```

- [ ] **Step 3: Build to verify**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: builds.

- [ ] **Step 4: Commit**

```bash
git add src/app/background.rs
git commit -m "feat(background): cache and invalidate PR number"
```

---

## Task 4: Render `#<n>` in the chip and return its rect

**Files:**
- Modify: `src/ui/dashboard/detail.rs` — `DetailDrawOutput` (lines 53-57), `render` (lines 121-159), `build_header_strip` (lines 396-465), header tests (lines 815-870).

- [ ] **Step 1: Write/extend the header-strip tests**

In `src/ui/dashboard/detail.rs`, replace the three existing header tests' `build_header_strip(...)` calls so they destructure the new `(Line, Option<HeaderChip>)` return and pass the new `pr_number` argument, and add a number test. The existing `header_strip_contains_all_chips_in_order` becomes (note the added `None` for `pr_number` after the lifecycle arg, and the `(line, _)` destructure):

```rust
    #[test]
    fn header_strip_contains_all_chips_in_order() {
        let theme = Theme::wsx();
        let (line, _) = build_header_strip(
            "repo-overview",
            "bakedbean/repo-overview",
            Some(BranchLifecycle::PrOpen),
            None,
            Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            2,
            Status::Question,
            Some(29),
            &theme,
            120,
        );
        let text = line_to_string(&line);
        assert!(text.contains("repo-overview"), "name missing: {text:?}");
        assert!(
            text.contains("bakedbean/repo-overview"),
            "branch missing: {text:?}"
        );
        assert!(
            text.contains("+12") && text.contains("−3"),
            "diff missing: {text:?}"
        );
        assert!(
            text.contains("● 2") || text.contains("2 procs"),
            "procs missing: {text:?}"
        );
        assert!(text.contains("?"), "status glyph missing: {text:?}");
        assert!(text.contains("29s"), "ago missing: {text:?}");
    }

    #[test]
    fn header_strip_omits_diff_when_none() {
        let theme = Theme::wsx();
        let (line, _) =
            build_header_strip("ws", "br", None, None, None, 0, Status::Idle, None, &theme, 80);
        let text = line_to_string(&line);
        assert!(!text.contains("+"), "diff cell should be absent: {text:?}");
        assert!(!text.contains("−"), "diff cell should be absent: {text:?}");
    }

    #[test]
    fn header_strip_omits_lifecycle_when_none() {
        let theme = Theme::wsx();
        let (line, chip) =
            build_header_strip("ws", "br", None, None, None, 0, Status::Idle, None, &theme, 80);
        let text = line_to_string(&line);
        let lower = text.to_lowercase();
        assert!(!lower.contains("pr open"), "no pr label: {text:?}");
        assert!(!lower.contains("merged"), "no pr label: {text:?}");
        assert!(chip.is_none(), "no chip rect when no lifecycle: {text:?}");
    }

    #[test]
    fn header_strip_shows_pr_number_and_reports_chip() {
        let theme = Theme::wsx();
        let (line, chip) = build_header_strip(
            "ws",
            "br",
            Some(BranchLifecycle::PrOpen),
            Some(152),
            None,
            0,
            Status::Idle,
            None,
            &theme,
            120,
        );
        let text = line_to_string(&line);
        assert!(text.contains("#152"), "pr number missing: {text:?}");
        let chip = chip.expect("chip rect should be present");
        // The chip span text is exactly "<glyph> #152 open"; width counts chars.
        assert_eq!(chip.width, "⏺ #152 open".chars().count());
        // start is the char-offset of the chip within the line; the text up to
        // it must be exactly that many chars.
        let prefix: String = text.chars().take(chip.start).collect();
        assert!(
            !prefix.contains("#152"),
            "chip.start should point at the chip, prefix was {prefix:?}"
        );
        assert!(
            text.chars().skip(chip.start).collect::<String>().starts_with("⏺ #152 open"),
            "chip.start should land on the chip glyph: {text:?}"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p wsx --lib dashboard::detail 2>&1 | tail -20`
Expected: FAIL — `HeaderChip` unknown and `build_header_strip` arity/return mismatch.

- [ ] **Step 3: Add `HeaderChip`, `pr_link_rect` output field, and rewrite `build_header_strip`**

In `src/ui/dashboard/detail.rs`, add the chip struct near the top of the file's item region (e.g. just above `DetailDrawOutput`):

```rust
/// Char-offset and char-width of the clickable PR chip within the header
/// line. `render` converts this into a screen `Rect`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HeaderChip {
    pub start: usize,
    pub width: usize,
}
```

Extend `DetailDrawOutput`:

```rust
#[derive(Debug, Default)]
pub struct DetailDrawOutput {
    pub chip_rects: Vec<ratatui::layout::Rect>,
    pub container_rects: [Option<ratatui::layout::Rect>; 4],
    pub pr_link_rect: Option<ratatui::layout::Rect>,
}
```

Rewrite `build_header_strip` to take `pr_number`, track a char-column counter, capture the chip, and return both. Replace the whole function body (lines 396-465):

```rust
/// One-line header strip at the top of the bar. Returns the rendered line
/// and, when a PR lifecycle chip was drawn, its char-offset + width so the
/// caller can make it clickable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_header_strip(
    name: &str,
    branch: &str,
    lifecycle: Option<BranchLifecycle>,
    pr_number: Option<u32>,
    diff: Option<DiffStats>,
    procs: u32,
    status: Status,
    ago_secs: Option<u64>,
    theme: &Theme,
    width: usize,
) -> (Line<'static>, Option<HeaderChip>) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut col: usize = 0;
    let mut pr_chip: Option<HeaderChip> = None;

    let gutter = GUTTER.to_string();
    col += gutter.chars().count();
    spans.push(Span::styled(gutter, theme.status_style(status)));

    col += 1;
    spans.push(Span::raw(" ".to_string()));

    col += name.chars().count();
    spans.push(Span::styled(
        name.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ));

    col += 2;
    spans.push(Span::raw("  ".to_string()));

    let branch_text = format!("⎇ {branch}");
    col += branch_text.chars().count();
    spans.push(Span::styled(branch_text, theme.dim_style()));

    if let Some(lc) = lifecycle {
        let (glyph, label) = lifecycle_chip(lc);
        if !glyph.is_empty() {
            col += 2;
            spans.push(Span::raw("  ".to_string()));
            let chip_text = match pr_number {
                Some(n) => format!("{glyph} #{n} {label}"),
                None => format!("{glyph} {label}"),
            };
            let chip_width = chip_text.chars().count();
            pr_chip = Some(HeaderChip {
                start: col,
                width: chip_width,
            });
            col += chip_width;
            spans.push(Span::styled(
                chip_text,
                theme
                    .lifecycle_style(Some(lc))
                    .unwrap_or_else(|| theme.dim_style()),
            ));
        }
    }

    if let Some(d) = diff
        && (d.added > 0 || d.removed > 0)
    {
        col += 2;
        spans.push(Span::raw("  ".to_string()));
        let added = format!("+{}", d.added);
        col += added.chars().count();
        spans.push(Span::styled(added, theme.ok_style()));
        col += 1;
        spans.push(Span::raw(" ".to_string()));
        let removed = format!("−{}", d.removed);
        col += removed.chars().count();
        spans.push(Span::styled(removed, theme.err_style()));
    }

    col += 2;
    spans.push(Span::raw("  ".to_string()));
    let procs_style = if procs > 0 {
        theme.status_style(Status::Thinking)
    } else {
        theme.dim_style()
    };
    let procs_text = format!("● {procs} procs");
    col += procs_text.chars().count();
    spans.push(Span::styled(procs_text, procs_style));

    col += 2;
    spans.push(Span::raw("  ".to_string()));
    let glyph = status.glyph().to_string();
    col += glyph.chars().count();
    spans.push(Span::styled(glyph, theme.status_style(status)));
    col += 1;
    spans.push(Span::raw(" ".to_string()));
    let label = status.label().to_string();
    col += label.chars().count();
    spans.push(Span::styled(label, theme.status_style(status)));

    let ago = format_ago_short(ago_secs);
    let ago_text = format!("  · {ago}");
    col += ago_text.chars().count();
    spans.push(Span::styled(ago_text, theme.dim_style()));

    let _ = (width, col);
    (Line::from(spans), pr_chip)
}
```

- [ ] **Step 4: Update `render` to consume the tuple and emit `pr_link_rect`**

In `src/ui/dashboard/detail.rs`, replace the header build + render (lines 121-132) with:

```rust
    let (header, pr_chip) = build_header_strip(
        &inputs.workspace.name,
        &inputs.workspace.branch,
        inputs.lifecycle,
        inputs.pr_number,
        inputs.diff,
        inputs.procs.len() as u32,
        inputs.status,
        inputs.ago_secs,
        theme,
        header_area.width as usize,
    );
    f.render_widget(Paragraph::new(header), header_area);

    let pr_link_rect = pr_chip.and_then(|c| {
        let x = header_area.x.saturating_add(c.start as u16);
        let right = header_area.x.saturating_add(header_area.width);
        if x >= right {
            return None;
        }
        let w = (c.width as u16).min(right - x);
        if w == 0 {
            return None;
        }
        Some(Rect {
            x,
            y: header_area.y,
            width: w,
            height: 1,
        })
    });
```

Then update the returned `DetailDrawOutput` (lines 155-158) to include it:

```rust
    DetailDrawOutput {
        chip_rects,
        container_rects,
        pr_link_rect,
    }
```

- [ ] **Step 5: Run the detail tests to verify they pass**

Run: `cargo test -p wsx --lib dashboard::detail 2>&1 | tail -30`
Expected: PASS, including `header_strip_shows_pr_number_and_reports_chip`.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(detail): render PR number in chip and report its rect"
```

---

## Task 5: Feed `pr_number` into the detail bar and store the chip rect

**Files:**
- Modify: `src/app/render.rs:371-397`

- [ ] **Step 1: Pass the cached PR number into the detail inputs**

In `src/app/render.rs`, change the `pr_number: None,` line (373) in the `DetailInputs { … }` literal to:

```rust
                            pr_number: app.pr_number.get(&ws.id).copied(),
```

- [ ] **Step 2: Store the returned chip rect against the workspace**

In `src/app/render.rs`, immediately after `app.detail_container_rects = out.container_rects;` (line 393), add:

```rust
                        app.pr_link_rect = out.pr_link_rect.map(|r| (ws.id, r));
```

- [ ] **Step 3: Build to verify**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: builds.

- [ ] **Step 4: Commit**

```bash
git add src/app/render.rs
git commit -m "feat(render): wire PR number into chip and capture its rect"
```

---

## Task 6: Open the PR on click

**Files:**
- Modify: `src/git/forge.rs` (`pr_web_argv`, `open_pr_in_browser` + test)
- Modify: `src/app/input.rs` (`open_pr_for_workspace` + mouse hit-test branch)

- [ ] **Step 1: Write the argv test**

In `src/git/forge.rs` test module, add:

```rust
    #[test]
    fn pr_web_argv_builds_expected() {
        assert_eq!(
            pr_web_argv("feature/foo"),
            vec![
                "pr".to_string(),
                "view".to_string(),
                "feature/foo".to_string(),
                "--web".to_string()
            ]
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p wsx --lib git::forge::tests::pr_web_argv_builds_expected 2>&1 | tail -10`
Expected: FAIL — `pr_web_argv` not found.

- [ ] **Step 3: Add `pr_web_argv` and `open_pr_in_browser`**

In `src/git/forge.rs` (non-test region, after `fetch_pr_status`):

```rust
/// The argv (after the `gh` program name) that opens `branch`'s PR in the
/// browser. Split out as a pure function so it can be unit-tested.
pub(crate) fn pr_web_argv(branch: &str) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        branch.to_string(),
        "--web".to_string(),
    ]
}

/// Open the PR for `branch` in the default browser via `gh pr view --web`.
/// Fire-and-forget: spawns detached and only logs spawn failures (gh itself
/// handles "no PR" / auth errors and we don't surface them on a click).
pub fn open_pr_in_browser(worktree: &Path, branch: &str) {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(pr_web_argv(branch))
        .current_dir(worktree)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Err(e) = cmd.spawn() {
        tracing::warn!(error = %e, branch, "failed to open PR in browser");
    }
}
```

- [ ] **Step 4: Run the forge tests to verify they pass**

Run: `cargo test -p wsx --lib git::forge 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Add the click handler and hit-test branch in `input.rs`**

In `src/app/input.rs`, add a helper near the other workspace helpers (e.g. after `attach_workspace`'s definition or alongside `open_change_modal`):

```rust
/// Open the selected workspace's PR in the browser. No-op if the workspace
/// id no longer resolves (e.g. removed between draw and click).
fn open_pr_for_workspace(app: &App, ws_id: crate::data::store::WorkspaceId) {
    if let Some((_, ws)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
        crate::git::forge::open_pr_in_browser(&ws.worktree_path, &ws.branch);
    }
}
```

Then add a branch to the `MouseEventKind::Down(MouseButton::Left)` `else if` chain. Insert it after the `agent_chip_rects` branch (which ends at line 2103, `}`) and before the `usage_graph_rect` branch (`} else if app.modal.is_none()`):

```rust
            } else if let Some((ws_id, _)) = app.pr_link_rect.filter(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking the PR chip opens the PR in the browser.
                open_pr_for_workspace(app, ws_id);
```

- [ ] **Step 6: Build and run the full test suite**

Run: `cargo test -p wsx 2>&1 | tail -30`
Expected: builds and all tests PASS.

- [ ] **Step 7: Commit**

```bash
git add src/git/forge.rs src/app/input.rs
git commit -m "feat(input): open PR in browser on chip click"
```

---

## Task 7: Manual verification & clippy

**Files:** none (verification only)

- [ ] **Step 1: Clippy**

Run: `cargo clippy -p wsx --all-targets 2>&1 | tail -30`
Expected: no new warnings from touched files.

- [ ] **Step 2: Format**

Run: `cargo fmt -p wsx`
Expected: no diff, or only formatting on touched files (commit if changed).

- [ ] **Step 3: Manual smoke test (documented for the executor)**

In a repo with an open PR on the current branch, launch `wsx`, select that workspace, and confirm the detail-bar header shows `⏺ #<n> open` (correct number, colored). Click the chip with the mouse and confirm the PR opens in the browser. Select a workspace with no PR and confirm no chip and no click effect.

- [ ] **Step 4: Commit any fmt/clippy fixes**

```bash
git add -A
git commit -m "chore: clippy/fmt for PR indicator"
```

---

## Self-Review Notes

- **Spec coverage:** number fetch+store (Tasks 1, 3), parallel `pr_number` map (Task 2), `#<n>` in chip (Task 4), chip rect tracking (Tasks 2, 4, 5), mouse hit-test + `gh pr view --web` open (Task 6), edge cases — NoPr→no chip (Task 4 guard + test), merged/closed clickable (rect emitted for any non-empty glyph), gh-missing degrade (existing poll behavior + spawn warn in Task 6). All covered.
- **Type consistency:** `pr_number` is `u32` end-to-end (`GhPrView.number: Option<u32>`, `PrStatus.number: Option<u32>`, `App.pr_number: HashMap<_, u32>`, `DetailInputs.pr_number: Option<u32>` already exists). `pr_link_rect` is `Option<(WorkspaceId, Rect)>` on `App` and `Option<Rect>` on `DetailDrawOutput`. `HeaderChip { start, width }` (both `usize`). Function names: `parse_gh_pr_status`, `fetch_pr_status`, `pr_web_argv`, `open_pr_in_browser`, `open_pr_for_workspace` — used consistently across tasks.
- **No placeholders:** every code step shows complete code; every run step has an expected result.
