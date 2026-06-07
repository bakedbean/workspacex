# Chronology Detail Line Numbers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a line-number gutter on the chronology detail peek — added (`+`) lines numbered from the change's resolved line, removed (`-`) lines with a blank gutter.

**Architecture:** `entry_lines` (pure) gains a `base_line: u32` param and renders each peek line with a 4-col right-aligned gutter (number for `+` lines, spaces for `-` lines). The renderer computes `base_line` for the expanded entry via the existing `resolve_line_in_file` (file IO stays out of the pure function).

**Tech Stack:** Rust, `ratatui`. Tests are `#[cfg(test)]` unit tests via `cargo test`.

**Builds on (verified current code):**
- `src/ui/chronology_bar.rs::entry_lines(ev: &ChangeEvent, worktree: &Path, expanded: bool, width: u16, highlight: EntryHighlight) -> Vec<Line<'static>>`. When `expanded`, it builds `peek: Vec<String>` — `Edit { old, new }` → up to 2 `format!("- {l}")` then up to 2 `format!("+ {l}")`; `Write { head }` → up to 3 `format!("+ {l}")`; `None` → empty — then pushes each, clipped to `width`, dimmed. The `EntryHighlight::Detail` arm reverses `out.iter_mut().skip(1)` (peek = everything after the header at index 0).
- The ONLY non-test caller is `src/ui/attached.rs::render_chronology_bar` (~line 340): `entry_lines(ev, draw.worktree, expanded, inner_width, highlight)`, inside a loop where `let expanded = Some(i) == draw.expanded;` and `ev` is `&ChangeEvent` (has `file_path: PathBuf`, `detail: ChangeDetail`).
- Test callers in `chronology_bar.rs`: `collapsed_entry_is_a_single_header_line`, `expanded_entry_adds_diff_peek_lines`, `header_highlight_reverses_first_line`, `detail_highlight_reverses_peek_lines_only`, `no_highlight_leaves_lines_unreversed` — each calls `entry_lines(ev(...), Path::new("/wt"), <expanded>, 40, EntryHighlight::X)`.
- `crate::activity::chronology::resolve_line_in_file(path: &Path, detail: &ChangeDetail) -> u32` (returns the 1-based line of the first non-blank `new` line; 1 for Write / not-found / unreadable).

---

## File Structure

- `src/ui/chronology_bar.rs` (modify) — `entry_lines` gains `base_line: u32`; peek lines get a gutter; tests updated + 2 new gutter tests.
- `src/ui/attached.rs` (modify) — compute `base_line` for the expanded entry and pass it.
- `README.md` (modify) — note the detail peek shows line numbers on added lines.

---

## Task 1: Gutter + `base_line` param in `entry_lines`

**Files:**
- Modify: `src/ui/chronology_bar.rs`
- Modify: `src/ui/attached.rs` (only the single `entry_lines` call site — arity bump with a placeholder, so the crate compiles; the real value is wired in Task 2)

- [ ] **Step 1: Write the failing tests**

In `src/ui/chronology_bar.rs`'s `#[cfg(test)] mod tests`, add a small text helper and two gutter tests:

```rust
    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn peek_numbers_added_lines_and_blanks_removed_gutter() {
        let ev = ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from("/wt/a.rs"),
            summary: String::new(),
            detail: ChangeDetail::Edit {
                old: "old0\nold1".into(),
                new: "new0\nnew1".into(),
            },
        };
        let lines = entry_lines(&ev, Path::new("/wt"), true, 60, 42, EntryHighlight::None);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        // out[0] header; out[1..3] removed (blank gutter); out[3..5] added (42, 43)
        assert!(texts[1].starts_with("     -"), "removed gutter blank: {:?}", texts[1]);
        assert!(texts[2].starts_with("     -"), "{:?}", texts[2]);
        assert!(texts[3].contains("42") && texts[3].contains("+ new0"), "{:?}", texts[3]);
        assert!(texts[4].contains("43") && texts[4].contains("+ new1"), "{:?}", texts[4]);
    }

    #[test]
    fn write_peek_numbers_from_base_line() {
        let ev = ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Write,
            file_path: PathBuf::from("/wt/a.rs"),
            summary: String::new(),
            detail: ChangeDetail::Write {
                head: "l1\nl2\nl3".into(),
            },
        };
        let lines = entry_lines(&ev, Path::new("/wt"), true, 60, 1, EntryHighlight::None);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts[1].contains("1") && texts[1].contains("+ l1"), "{:?}", texts[1]);
        assert!(texts[2].contains("2") && texts[2].contains("+ l2"), "{:?}", texts[2]);
        assert!(texts[3].contains("3") && texts[3].contains("+ l3"), "{:?}", texts[3]);
    }
```

Also UPDATE the 5 existing `entry_lines(...)` calls in this test module to insert the new `base_line` argument (use `1`) as the 5th argument, before `EntryHighlight::...`. For example:

```rust
        let lines = entry_lines(
            &ev("/wt/src/main.rs", "fn foo()"),
            Path::new("/wt"),
            false,
            40,
            1,
            EntryHighlight::None,
        );
```

Apply the same insertion to `expanded_entry_adds_diff_peek_lines`, `header_highlight_reverses_first_line`, `detail_highlight_reverses_peek_lines_only`, and `no_highlight_leaves_lines_unreversed`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib chronology_bar`
Expected: FAIL — arity mismatch (`entry_lines` takes 5 args) / the two new tests don't compile yet.

- [ ] **Step 3: Add the `base_line` param and gutter**

In `src/ui/chronology_bar.rs`, change the signature (add `base_line: u32` after `width`):

```rust
pub fn entry_lines(
    ev: &ChangeEvent,
    worktree: &Path,
    expanded: bool,
    width: u16,
    base_line: u32,
    highlight: EntryHighlight,
) -> Vec<Line<'static>> {
```

Replace the `if expanded { ... }` peek block with a gutter-aware version:

```rust
    if expanded {
        // (line number, marker, text). `+` (added) lines carry a current-file
        // line number starting at base_line; `-` (removed) lines have none
        // (they no longer exist in the file).
        let mut peek: Vec<(Option<u32>, char, String)> = Vec::new();
        match &ev.detail {
            ChangeDetail::Edit { old, new } => {
                for l in old.lines().take(2) {
                    peek.push((None, '-', l.to_string()));
                }
                for (k, l) in new.lines().take(2).enumerate() {
                    peek.push((Some(base_line.saturating_add(k as u32)), '+', l.to_string()));
                }
            }
            ChangeDetail::Write { head } => {
                for (k, l) in head.lines().take(3).enumerate() {
                    peek.push((Some(base_line.saturating_add(k as u32)), '+', l.to_string()));
                }
            }
            ChangeDetail::None => {}
        }
        for (gutter, marker, text) in peek {
            // 4-col right-aligned number + a space, then marker + text. Removed
            // lines use a 5-space blank gutter so columns line up.
            let line = match gutter {
                Some(n) => format!("{n:>4} {marker} {text}"),
                None => format!("     {marker} {text}"),
            };
            let clipped: String = line.chars().take(width as usize).collect();
            out.push(Line::from(Span::styled(
                clipped,
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
    }
```

The `match highlight { ... }` block below is unchanged (the `Detail` arm still reverses `out.iter_mut().skip(1)`). Update the doc comment above `entry_lines` to mention the numbered gutter, e.g. append: "Peek lines carry a line-number gutter: added (`+`) lines are numbered from `base_line`, removed (`-`) lines have a blank gutter."

- [ ] **Step 4: Bump the attached.rs call site (placeholder)**

In `src/ui/attached.rs::render_chronology_bar`, the call is:

```rust
        let lines = crate::ui::chronology_bar::entry_lines(
            ev,
            draw.worktree,
            expanded,
            inner_width,
            highlight,
        );
```

Insert `1,` as the `base_line` argument before `highlight` so the crate compiles (the real computation lands in Task 2):

```rust
        let lines = crate::ui::chronology_bar::entry_lines(
            ev,
            draw.worktree,
            expanded,
            inner_width,
            1,
            highlight,
        );
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib chronology_bar`
Expected: PASS — the 2 new gutter tests plus the updated existing tests.
Run: `cargo build` — zero warnings. `cargo fmt` then `cargo fmt --check` — clean (only the two files; revert out-of-scope via `git checkout`).

- [ ] **Step 6: Commit**

```bash
git add src/ui/chronology_bar.rs src/ui/attached.rs
git commit -m "feat(chronology): line-number gutter on detail peek (+lines numbered, -blank)"
```

---

## Task 2: Compute the real `base_line` in the renderer

**Files:**
- Modify: `src/ui/attached.rs`

No isolated unit test (file IO + render side-effects); `resolve_line_in_file` is already tested, and the gutter formatting is tested in Task 1. Verified by build + manual.

- [ ] **Step 1: Compute `base_line` for the expanded entry**

In `src/ui/attached.rs::render_chronology_bar`, just before the `entry_lines(...)` call (after `let highlight = ...;`), add:

```rust
        // Number the detail peek from the change's resolved line; only the
        // expanded entry shows a peek, so only it needs the (file-reading) lookup.
        let base_line = if expanded {
            crate::activity::chronology::resolve_line_in_file(&ev.file_path, &ev.detail)
        } else {
            1
        };
```

Then change the `entry_lines(...)` call's `base_line` argument from the placeholder `1` to `base_line`:

```rust
        let lines = crate::ui::chronology_bar::entry_lines(
            ev,
            draw.worktree,
            expanded,
            inner_width,
            base_line,
            highlight,
        );
```

- [ ] **Step 2: Build + manual verify**

Run: `cargo build` — zero warnings. `cargo test --lib` — no regressions. `cargo fmt --check` — clean.
Manual: in an attached Claude workspace, expand a chronology entry — the diff peek's `+` lines show right-aligned line numbers matching the file; `-` lines have a blank gutter; clicking/Enter still opens the editor at the same first `+` line number.

- [ ] **Step 3: Commit**

```bash
git add src/ui/attached.rs
git commit -m "feat(chronology): number the detail peek from the change's resolved line"
```

---

## Task 3: README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the gutter**

In the "Change chronology" section of `README.md` where the expandable detail / diff peek is described, add a sentence: expanding an entry shows a short diff peek with a line-number gutter — added lines are numbered with their current file line (matching where "open in editor" jumps), removed lines have a blank gutter. Match the existing prose style.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: note line-number gutter in the chronology detail peek"
```

---

## Self-Review (completed during planning)

**Spec coverage:**
- Numbered `+` lines, blank gutter on `-` lines (Option A) → Task 1 (gutter tuples + format). ✓
- `base_line` from `resolve_line_in_file` (agrees with editor jump) → Task 2. ✓
- `entry_lines` stays pure; renderer does the IO → Task 1 (param) + Task 2 (compute). ✓
- IO only for the expanded entry → Task 2 (`if expanded`). ✓
- Gutter preserved on clip (text tail trimmed) → Task 1 (compose full line incl. gutter, then `chars().take(width)`). ✓
- `Detail` highlight still reverses peek lines → Task 1 (highlight block unchanged; gutter is part of each peek span). ✓
- README → Task 3. ✓

**Placeholder scan:** No vague steps; every code step shows complete code. The Task 1→Task 2 placeholder (`1`) is an explicit, named compile-bridge, replaced in Task 2.

**Type consistency:** `entry_lines(ev, worktree, expanded, width, base_line: u32, highlight)` arity is consistent across the call site (attached.rs) and all 7 test calls; `resolve_line_in_file(&ev.file_path, &ev.detail) -> u32` matches the `base_line` type; `line_text` helper used only in the new tests.
