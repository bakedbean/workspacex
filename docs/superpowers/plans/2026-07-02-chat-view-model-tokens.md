# Chat-view model + token usage — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a combined `opus 4.8 45k/200k` model + token-usage element in the attached chat view's right-justified bottom-line block, alongside the existing procs / diff / PR elements.

**Architecture:** Two new pure formatters in `src/detail_modules/session_summary.rs` (reusing the existing `abbreviate_tokens` / `resolve_window` helpers so the chat view and dashboard stay in lockstep) produce a `(text, warn)` pair. That pair is computed at the render call site from data already in scope (`app.workspace_events`) and threaded through `render_panes` → `render_chip_row` as a new optional parameter, where it becomes the leftmost (lowest-priority, dropped-first) element of the flush-right block.

**Tech Stack:** Rust, ratatui (TUI), existing `TestBackend` render-test pattern.

## Global Constraints

- CI runs rustfmt, clippy, and tests as **separate** gates. Before every commit run all three: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. (Clippy passing does NOT imply fmt is clean.)
- `click_chip_auto_spawns_session_when_missing` is a known flaky PTY-timing test — if it is the *only* failure, re-run; do not treat it as a regression.
- Do NOT modify anything under `/home/eben/sessionx` (read-only source of the `sessionx` build dependency). `WorkspaceEvents` already exposes `context_tokens: Option<u64>` and `model_id: Option<String>`; no changes to it are needed.
- Model label chosen format: family + version, e.g. `opus 4.8`. Token format: compact `used/window` (e.g. `45k/200k`), no spaces around the slash. Combined element is a single unit, dropped whole under width pressure.
- Warn thresholds mirror the detail bar exactly: fill ≥ 85% of a resolvable window, or raw tokens ≥ 150_000 when the window is unknown.

---

## File Structure

- `src/detail_modules/session_summary.rs` — **modify.** Add `short_model_label` (model-id → label parser) and `format_chip_model_tokens` (WorkspaceEvents → `(text, warn)`), plus unit tests. Lives here beside the existing `abbreviate_tokens` / `resolve_window` / `format_context_line` helpers it reuses.
- `src/ui/attached/chip_row.rs` — **modify.** Add a `model_tokens: Option<(String, bool)>` parameter to `render_chip_row`, a `model_tokens_chip_parts` builder, push it as the leftmost block element; update existing test call sites and add new render tests.
- `src/ui/attached/mod.rs` — **modify.** Add the same parameter to `render_panes`, pass it through to `render_chip_row`; update the in-module test caller.
- `src/app/render.rs` — **modify.** Compute `model_tokens` for the focused workspace and pass it to `render_panes`; pass `None` from the PM branch.
- `src/ui/dashboard/detail.rs` — **modify.** Pass `None` at its `render_chip_row` call (the detail bar's chip row carries pinned commands only).

---

## Task 1: `short_model_label` model-id parser

**Files:**
- Modify: `src/detail_modules/session_summary.rs` (add fn near `resolve_window`, ~line 189; add tests in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing (pure string function).
- Produces: `pub(crate) fn short_model_label(model_id: &str) -> String` — used by Task 2.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/detail_modules/session_summary.rs`:

```rust
    #[test]
    fn short_model_label_parses_family_and_version() {
        assert_eq!(short_model_label("claude-opus-4-8"), "opus 4.8");
        assert_eq!(short_model_label("claude-sonnet-5"), "sonnet 5");
        assert_eq!(short_model_label("claude-haiku-4-5"), "haiku 4.5");
    }

    #[test]
    fn short_model_label_strips_bracketed_variant() {
        assert_eq!(short_model_label("claude-opus-4-8[1m]"), "opus 4.8");
    }

    #[test]
    fn short_model_label_ignores_trailing_date_segment() {
        // The date segment (>2 digits) is not part of the version.
        assert_eq!(short_model_label("claude-haiku-4-5-20251001"), "haiku 4.5");
    }

    #[test]
    fn short_model_label_falls_back_for_unknown_ids() {
        // No known family word: strip a leading "claude-" and truncate to 12.
        assert_eq!(short_model_label("gpt-5-codex"), "gpt-5-codex");
        assert_eq!(
            short_model_label("some-really-long-unknown-model-id"),
            "some-really-"
        );
    }

    #[test]
    fn short_model_label_family_without_version() {
        assert_eq!(short_model_label("claude-opus"), "opus");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx short_model_label`
Expected: FAIL — `cannot find function short_model_label in this scope`.

- [ ] **Step 3: Implement `short_model_label`**

Add just below `resolve_window` (after line ~189) in `src/detail_modules/session_summary.rs`:

```rust
/// A short display label for a model id: the Claude family word plus its
/// version, e.g. `claude-opus-4-8[1m]` → `opus 4.8`, `claude-sonnet-5` →
/// `sonnet 5`. The version is the run of short (1-2 digit) numeric segments
/// right after the family, so a trailing date segment like `20251001` is
/// ignored. Unknown / non-Claude ids fall back to the id with any leading
/// `claude-` stripped, truncated to 12 chars.
pub(crate) fn short_model_label(model_id: &str) -> String {
    // Drop a trailing bracketed variant tag like "[1m]".
    let base = model_id.split('[').next().unwrap_or(model_id);
    let segments: Vec<&str> = base.split('-').collect();
    let family_pos = segments
        .iter()
        .position(|s| matches!(*s, "opus" | "sonnet" | "haiku"));
    match family_pos {
        Some(i) => {
            let family = segments[i];
            let is_short_numeric =
                |s: &str| !s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit());
            let version: Vec<&str> = segments[i + 1..]
                .iter()
                .copied()
                .take_while(|s| is_short_numeric(s))
                .collect();
            if version.is_empty() {
                family.to_string()
            } else {
                format!("{} {}", family, version.join("."))
            }
        }
        None => {
            let cleaned = base.strip_prefix("claude-").unwrap_or(base);
            cleaned.chars().take(12).collect()
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx short_model_label`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add src/detail_modules/session_summary.rs
git commit -m "feat(session-summary): add short_model_label model-id parser

Claude-Session: https://claude.ai/code/session_01VFfzqBnQoCKC1u8aGKHyJJ"
```

---

## Task 2: `format_chip_model_tokens` combined formatter

**Files:**
- Modify: `src/detail_modules/session_summary.rs` (add fn near `format_context_line`, ~line 213; add tests)

**Interfaces:**
- Consumes: `short_model_label` (Task 1); existing `abbreviate_tokens`, `resolve_window`; `crate::activity::events::WorkspaceEvents`.
- Produces: `pub(crate) fn format_chip_model_tokens(evt: &WorkspaceEvents) -> Option<(String, bool)>` — the chip text and warn flag, `None` when there's no token data. Used by Task 3.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block (the tests already build `WorkspaceEvents` via `..WorkspaceEvents::default()` — reuse that pattern, see `format_context_line_*` tests):

```rust
    #[test]
    fn format_chip_model_tokens_known_window() {
        let evt = WorkspaceEvents {
            context_tokens: Some(45_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "opus 4.8 45k/200k");
        assert!(!warn);
    }

    #[test]
    fn format_chip_model_tokens_warns_past_85_percent() {
        let evt = WorkspaceEvents {
            context_tokens: Some(190_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "opus 4.8 190k/200k");
        assert!(warn);
    }

    #[test]
    fn format_chip_model_tokens_unknown_window_shows_raw_tokens() {
        let evt = WorkspaceEvents {
            context_tokens: Some(77_000),
            model_id: Some("gpt-5-codex".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "gpt-5-codex 77k");
        assert!(!warn);
    }

    #[test]
    fn format_chip_model_tokens_unknown_window_warns_past_150k() {
        let evt = WorkspaceEvents {
            context_tokens: Some(160_000),
            model_id: Some("gpt-5-codex".to_string()),
            ..WorkspaceEvents::default()
        };
        let (_text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert!(warn);
    }

    #[test]
    fn format_chip_model_tokens_none_when_no_tokens() {
        let evt = WorkspaceEvents {
            context_tokens: None,
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        assert!(format_chip_model_tokens(&evt).is_none());
        let zero = WorkspaceEvents {
            context_tokens: Some(0),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        assert!(format_chip_model_tokens(&zero).is_none());
    }

    #[test]
    fn format_chip_model_tokens_tokens_only_when_no_model() {
        let evt = WorkspaceEvents {
            context_tokens: Some(45_000),
            model_id: None,
            ..WorkspaceEvents::default()
        };
        let (text, _warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "45k");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx format_chip_model_tokens`
Expected: FAIL — `cannot find function format_chip_model_tokens in this scope`.

- [ ] **Step 3: Implement `format_chip_model_tokens`**

Add just below `format_context_line` (after ~line 213) in `src/detail_modules/session_summary.rs`:

```rust
/// The chat view's compact model + token-usage chip: `{label} {used}/{window}`
/// when the window is resolvable (e.g. `opus 4.8 45k/200k`), else
/// `{label} {used}` (raw tokens). The model label is omitted when `model_id`
/// is absent. Returns `(text, warn)`; `warn` mirrors the detail bar
/// (`format_context_line`): fill ≥ 85% of a known window, or raw tokens
/// ≥ 150k when the window is unknown. `None` when there's no token data.
pub(crate) fn format_chip_model_tokens(evt: &WorkspaceEvents) -> Option<(String, bool)> {
    let n = evt.context_tokens.filter(|&n| n > 0)?;
    let label = evt.model_id.as_deref().map(short_model_label);
    let (tokens_text, warn) = match resolve_window(n, evt.model_id.as_deref()) {
        Some(w) => {
            let pct = (n.saturating_mul(100) / w).min(999);
            (
                format!("{}/{}", abbreviate_tokens(n), abbreviate_tokens(w)),
                pct >= 85,
            )
        }
        None => (abbreviate_tokens(n), n >= 150_000),
    };
    let text = match label {
        Some(l) => format!("{l} {tokens_text}"),
        None => tokens_text,
    };
    Some((text, warn))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx format_chip_model_tokens`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings
git add src/detail_modules/session_summary.rs
git commit -m "feat(session-summary): add format_chip_model_tokens for chip row

Claude-Session: https://claude.ai/code/session_01VFfzqBnQoCKC1u8aGKHyJJ"
```

---

## Task 3: Render the element and wire all callers

This task changes the arity of `render_chip_row` and `render_panes`, so it must update **every** caller in the same commit for the crate to compile. Deliverable: the element renders leftmost in the block, drops first when narrow, and the full suite passes.

**Files:**
- Modify: `src/ui/attached/chip_row.rs` (new param + builder + push element; update 9 existing test call sites; add 4 new render tests)
- Modify: `src/ui/attached/mod.rs` (`render_panes` param + pass-through at line ~148; update in-module test caller at line ~420)
- Modify: `src/app/render.rs` (compute `model_tokens` at ~line 454; pass to `render_panes` at line ~567; pass `None` at PM call ~line 642)
- Modify: `src/ui/dashboard/detail.rs` (pass `None` at `render_chip_row`, line 168)

**Interfaces:**
- Consumes: `crate::detail_modules::session_summary::format_chip_model_tokens` (Task 2).
- Produces: `render_chip_row(f, area, pinned, procs, diff, pr, model_tokens: Option<(String, bool)>, theme)` and `render_panes(..., pr, model_tokens: Option<(String, bool)>, agents, active_agent, theme)`.

- [ ] **Step 1: Write the failing render tests**

Add to the `#[cfg(test)] mod tests` block in `src/ui/attached/chip_row.rs`:

```rust
    #[test]
    fn render_chip_row_paints_model_tokens_leftmost() {
        // The combined model+token element sits leftmost in the flush-right
        // block: model+tokens, then procs, then diff, then the PR chip.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    3,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some((BranchLifecycle::PrOpen, 152)),
                    Some(("opus 4.8 45k/200k".to_string(), false)),
                    &theme,
                );
                pr_rect = r;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip present and fits an 80-wide row");
        let buf = terminal.backend().buffer();
        let block = "opus 4.8 45k/200k ● 3p +12 −3 ⏺ #152 open";
        let block_w = block.chars().count() as u16;
        let start = rect.x + rect.width - block_w;
        let mut painted = String::new();
        for x in start..start + block_w {
            painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(painted, block);
        assert_eq!(rect.x + rect.width, 80, "PR chip stays flush-right");
    }

    #[test]
    fn render_chip_row_shows_model_tokens_without_others() {
        // Before any procs/diff/PR, the model+token element still shows on its
        // own, flush to the right edge.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    None,
                    Some(("opus 4.8 45k/200k".to_string(), false)),
                    &theme,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let text = "opus 4.8 45k/200k";
        let w = text.chars().count() as u16;
        let start = 80 - w;
        let mut painted = String::new();
        for x in start..start + w {
            painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(painted, text);
    }

    #[test]
    fn render_chip_row_drops_model_tokens_first_when_narrow() {
        // On a row too narrow for the whole block, the model+token element is
        // dropped before the PR chip (it is leftmost / lowest priority).
        let theme = Theme::wsx();
        // "⏺ #9 open" = 9 cells; + "  " rule gap → 11. " 1 pr " chip = 6 cells.
        // Width 20 fits chip + gap + PR but not a leading model+token element.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 20, 1);
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some((BranchLifecycle::PrOpen, 9)),
                    Some(("opus 4.8 45k/200k".to_string(), false)),
                    &theme,
                );
                pr_rect = r;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip kept when model+tokens is dropped");
        let buf = terminal.backend().buffer();
        let mut pr_painted = String::new();
        for x in rect.x..rect.x + rect.width {
            pr_painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(pr_painted, "⏺ #9 open");
        let row: String = (0..20).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(!row.contains("opus"), "model+tokens should be dropped: {row:?}");
    }

    #[test]
    fn render_chip_row_model_tokens_warn_style() {
        // When the warn flag is set, the element paints in the theme warn color.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    None,
                    Some(("opus 4.8 190k/200k".to_string(), true)),
                    &theme,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // The first painted cell of the element carries the warn foreground.
        let text = "opus 4.8 190k/200k";
        let w = text.chars().count() as u16;
        let start = 80 - w;
        assert_eq!(buf[(start, 0)].fg, theme.warn_style().fg.unwrap());
    }
```

- [ ] **Step 2: Run tests to verify they fail (compile error)**

Run: `cargo test -p wsx --lib chip_row 2>&1 | head -20`
Expected: FAIL — compile error, `render_chip_row` takes 7 arguments but 8 were supplied (the new tests pass the not-yet-added `model_tokens` arg).

- [ ] **Step 3: Add the element builder to `chip_row.rs`**

Add near the other `*_chip_parts` helpers (after `procs_chip_parts`, ~line 57) in `src/ui/attached/chip_row.rs`:

```rust
/// Build the combined `{model} {tokens}` element (e.g. `opus 4.8 45k/200k`)
/// plus its column width, or `None` when there's no token data. Warn-colored
/// (matching the detail bar) when `warn` is set, else dim. This is the
/// leftmost / lowest-priority element in the flush-right block.
fn model_tokens_chip_parts(
    model_tokens: Option<(String, bool)>,
    theme: &Theme,
) -> Option<(Vec<Span<'static>>, usize)> {
    let (text, warn) = model_tokens?;
    if text.is_empty() {
        return None;
    }
    let width = text.chars().count();
    let style = if warn {
        theme.warn_style()
    } else {
        theme.dim_style()
    };
    Some((vec![Span::styled(text, style)], width))
}
```

- [ ] **Step 4: Add the parameter and push the element**

In `src/ui/attached/chip_row.rs`, change the `render_chip_row` signature (line ~121) to insert `model_tokens` after `pr`:

```rust
pub(crate) fn render_chip_row(
    f: &mut Frame,
    area: Rect,
    pinned: &[PinnedCommand],
    procs: u32,
    diff: Option<crate::git::DiffStats>,
    pr: Option<(BranchLifecycle, u32)>,
    model_tokens: Option<(String, bool)>,
    theme: &Theme,
) -> (Vec<Rect>, Option<Rect>) {
```

Then, where `elements` is built (line ~164), bump the capacity to 4 and push the model+tokens element **first** (before procs):

```rust
    // The optional elements in left-to-right order. Each is `(spans, width)`.
    let mut elements: Vec<(Vec<Span<'static>>, usize)> = Vec::with_capacity(4);
    if let Some(parts) = model_tokens_chip_parts(model_tokens, theme) {
        elements.push(parts);
    }
    if let Some(parts) = procs_chip_parts(procs, theme) {
        elements.push(parts);
    }
```

(Leave the `diff` and `pr` pushes below unchanged.)

- [ ] **Step 5: Update the 9 existing test call sites in `chip_row.rs`**

Every existing `render_chip_row(...)` call in the test module must gain `None,` as the new 7th argument (after the `pr` argument, before `theme`). The two one-liners:

At line ~551, replace:
```rust
                render_chip_row(f, area, &pinned, 2, None, None, &theme);
```
with:
```rust
                render_chip_row(f, area, &pinned, 2, None, None, None, &theme);
```

At line ~576, replace:
```rust
                render_chip_row(f, area, &pinned, 0, None, None, &theme);
```
with:
```rust
                render_chip_row(f, area, &pinned, 0, None, None, None, &theme);
```

For the multi-line calls (at lines ~335, ~369, ~396, ~442, ~478, ~511, ~599), insert a `None,` line immediately after the `pr` argument line (the `Some((BranchLifecycle::..., N)),` or `None,` that precedes `&theme,`). Example — at line ~335 the args become:

```rust
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some((BranchLifecycle::PrOpen, 152)),
                    None,
                    &theme,
                );
```

Apply the same insertion to each remaining multi-line caller.

- [ ] **Step 6: Thread the parameter through `render_panes` in `mod.rs`**

In `src/ui/attached/mod.rs`, add `model_tokens` to the `render_panes` signature after `pr` (line ~91):

```rust
    pr: Option<(BranchLifecycle, u32)>,
    model_tokens: Option<(String, bool)>,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
```

Pass it through at the `render_chip_row` call (line ~148):

```rust
    let (chip_rects, pr_link_rect) =
        render_chip_row(f, chips_area, pinned, procs, diff, pr, model_tokens, theme);
```

Update the in-module test caller (`render_panes_draws_info_on_top_and_full_width_separator`, line ~420): insert `None,` after the `pr` argument (`None,` at line ~434) so the arg list stays valid:

```rust
                0,
                None,
                None,
                None,
                &[],
                None,
                &theme,
```

(The three `None`s are diff, pr, and the new model_tokens; procs is the preceding `0`.)

- [ ] **Step 7: Compute and pass `model_tokens` in `render.rs`**

In `src/app/render.rs`, in the focused-attached branch, after the `procs` derivation (~line 458) add:

```rust
            // Model + token usage for the chip row's leftmost element, sourced
            // from the same events the dashboard SESSION SUMMARY reads, so the
            // chat-view chip and the detail bar stay in lockstep.
            let model_tokens = app
                .workspace_events
                .get(&focused_id)
                .and_then(crate::detail_modules::session_summary::format_chip_model_tokens);
```

Pass it to `render_panes` (call at line ~567) — insert after the `pr` argument:

```rust
                procs,
                diff,
                pr,
                model_tokens,
                &focused_agents_list,
                active_agent,
                &app.theme,
```

In the `AttachedPm` branch's `render_panes` call (line ~642), insert `None,` after its `pr` argument (`None,`):

```rust
                pinned,
                0,
                None,
                None,
                None,
                &[],
                None,
                &app.theme,
```

(procs `0`, diff `None`, pr `None`, model_tokens `None`.)

- [ ] **Step 8: Pass `None` at the dashboard detail chip row**

In `src/ui/dashboard/detail.rs` (line 168), the detail bar's chip row carries pinned commands only:

```rust
        crate::ui::attached::render_chip_row(f, area, inputs.pinned, 0, None, None, None, theme).0
```

- [ ] **Step 9: Run the new render tests**

Run: `cargo test -p wsx --lib chip_row`
Expected: PASS — including the 4 new `render_chip_row_*model_tokens*` tests.

- [ ] **Step 10: Full gate + commit**

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```
Expected: all pass. (If `click_chip_auto_spawns_session_when_missing` is the only failure, re-run — it is a known flaky PTY test.)

```bash
git add src/ui/attached/chip_row.rs src/ui/attached/mod.rs src/app/render.rs src/ui/dashboard/detail.rs
git commit -m "feat(attached): show model + token usage in chat-view bottom line

Adds a combined 'opus 4.8 45k/200k' element as the leftmost item of the
chip-row's flush-right block, warn-colored past the detail bar's thresholds
and dropped first under width pressure.

Claude-Session: https://claude.ai/code/session_01VFfzqBnQoCKC1u8aGKHyJJ"
```

---

## Manual verification

After Task 3, sanity-check in the running app (optional but recommended):

- [ ] Launch wsx, attach to a workspace whose agent has produced token usage. Confirm the bottom line shows `{model} {used}/{window}` at the left of the right-justified block, before `● Np` / `+A −R` / the PR chip.
- [ ] Narrow the terminal until the block no longer fits — confirm the model+token element disappears first while the PR chip persists.
- [ ] Confirm a workspace with no token data yet shows nothing new (no bare element).

---

## Self-review notes

- **Spec coverage:** display element (Task 3), text format + model label (Tasks 1–2), visibility/omit rules (Task 2 `None` cases + `model_tokens_chip_parts` empty guard), warn color/thresholds (Task 2 + Task 3 warn test), drop-first behavior (Task 3 narrow test), plumbing incl. PM/detail `None` (Task 3 Steps 7–8). All covered.
- **Placeholder scan:** none — every code step contains full code.
- **Type consistency:** `format_chip_model_tokens -> Option<(String, bool)>` produced in Task 2, consumed as the `model_tokens: Option<(String, bool)>` parameter in Task 3; `short_model_label(&str) -> String` produced in Task 1, consumed in Task 2. Consistent throughout.
