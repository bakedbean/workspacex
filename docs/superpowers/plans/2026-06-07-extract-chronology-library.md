# Extract Chronology Library Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the change-chronology feature out of `wsx` into a new standalone crate `chronox` (core + optional `ratatui` UI feature) in its own git repo, and have `wsx` consume it as a git dependency with no change in on-screen behaviour.

**Architecture:** Build the crate first in a fresh repo, porting the existing five modules with their ~400 lines of tests. Three seams are cut while moving: (1) the syntax tokenizer is split into a ratatui-free core emitting neutral `TokenKind` tokens plus a feature-gated ratatui renderer; (2) config resolution is inverted onto a `ConfigSource` trait instead of `wsx`'s `Store`/`Repo`; (3) two `activity/events.rs` helpers are inlined. Then `wsx` deletes the moved modules, adds the git dependency, supplies a `ConfigSource` adapter, and updates import paths. Everything else is a mechanical file move.

**Tech Stack:** Rust (edition 2024), `serde`/`serde_json`, `ratatui` 0.29 (optional feature), `log` facade, `tempfile` (dev). Tests via `cargo test`; both `--no-default-features` (core) and `--all-features` (core + ratatui) must pass.

**Cross-repo iteration note:** During Phase B, point `wsx` at the crate with a **path** dependency (or a `[patch]`) so you can iterate without pushing. The final task replaces the path with a pinned `git` + `rev`. Never leave the path dependency in a committed `wsx` `Cargo.toml`.

**Source of truth for the move (verify against the live files before copying):**
- `src/activity/chronology.rs` — `ChangeEvent { timestamp_ms, tool, file_path, summary, detail, source }`, `ChangeDetail { Edit{old,new}, Write{head}, None }`, `ChangeTool { Edit, MultiEdit, Write, NotebookEdit }`, `ChangeSource { session_file, line_index, index_in_line }`, `DETAIL_MAX_CHARS`, `extract_change_events(v, detail_max)`, `parse_file(path)`, `load_full_change(ev)`, `resolve_line_in_file(path, detail)`, the session-file discovery fn, and the `Timeline` type (`refresh`, `events`). Uses `encode_cwd` + `parse_iso8601_ms` from `src/activity/events.rs`.
- `src/ui/chronology_nav.rs` — `NavKey`, `NavAction`, `nav(sel, key, len)`, `adjust_scroll(..)`, `clamp_scroll(scroll, len, body)`.
- `src/ui/chronology_bar.rs` — `entry_lines(ev, worktree, width, selected)`, `relative_display`, `hhmm`, `abbreviate_path`, `should_auto_hide`.
- `src/ui/syntax.rs` — `LangSpec`, `lang_for_path`, `highlight_code(text, spec) -> Vec<Span>`, `change_detail_lines_styled(detail, base_line, lang) -> Vec<Line>`, `clip_line_to_width(line, width)`, the `*_style()` colour fns.
- `src/config/chronology.rs` — `ChronologyConfig`, `ChronologyOverride`, `Side`, `WidthSpec`, `with_override`, `sanitize`, `resolved_width`, `resolve(repo, store)`, `resolve_global_only(store)`.
- Consumers in `wsx`: `src/app.rs` (`chronology: HashMap<WorkspaceId, Timeline>`, `refresh_chronology`), `src/app/render.rs` (modal render calling `change_detail_lines_styled` + `clamp_scroll`), `src/app/input.rs` (bar nav + open-modal), `src/ui/attached.rs` (`render_chronology_bar`, `ChronologyDraw`, `ChronologyHits`), `src/ui/modal.rs` (`Modal::ChangeDetail`).

---

## File Structure

**New repo `chronox/`:**
- `Cargo.toml` — deps, `ratatui` optional feature, `default = ["ratatui"]`.
- `src/lib.rs` — module decls + re-exports; documents core vs ratatui split.
- `src/event.rs` — `ChangeEvent`, `ChangeDetail`, `ChangeTool`, `ChangeSource` [core].
- `src/extract.rs` — parsing, `load_full_change`, `resolve_line_in_file`, session discovery, inlined `encode_cwd`/`parse_iso8601_ms` [core].
- `src/timeline.rs` — `Timeline` cache [core].
- `src/nav.rs` — `nav`, `adjust_scroll`, `clamp_scroll`, `NavKey`, `NavAction` [core].
- `src/syntax.rs` — `LangSpec`, `lang_for_path`, `tokenize_line`, `TokenKind`, `Token`, `DiffLine`, `DiffMarker`, `change_detail_diff` [core].
- `src/render.rs` — `style_for`, `change_detail_lines_styled`, `clip_line_to_width`, `entry_lines` [ratatui feature].
- `src/config.rs` — config types + `ConfigSource` trait + `resolve`/`resolve_global_only` [core].

**`wsx/` (consumer) — modified:**
- `Cargo.toml` — add git dependency.
- Delete: `src/activity/chronology.rs`, `src/ui/chronology_nav.rs`, `src/ui/chronology_bar.rs`, `src/ui/syntax.rs`, `src/config/chronology.rs`.
- `src/activity/mod.rs`, `src/ui/mod.rs`, `src/config/mod.rs` — drop deleted modules.
- New `src/config/chronology_source.rs` — `ConfigSource` adapter over `Store`/`Repo`.
- `src/app.rs`, `src/app/input.rs`, `src/app/render.rs`, `src/ui/attached.rs`, `src/ui/modal.rs` — update `use` paths to the crate.

---

## Task 1: Scaffold the crate repo

**Files:**
- Create: `chronox/Cargo.toml`
- Create: `chronox/src/lib.rs`
- Create: `chronox/.gitignore`

The crate lives in its own repo at `~/chronox` (already created).

- [ ] **Step 1: Initialize the repo and create src/**

```bash
mkdir -p ~/chronox/src
cd ~/chronox && git init -q
```

`chronox/Cargo.toml`:

```toml
[package]
name = "chronox"
version = "0.1.0"
edition = "2024"
description = "Parse Claude Code session logs into a navigable timeline of file changes, with an optional ratatui UI."
license = "MIT OR Apache-2.0"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
ratatui = { version = "0.29", optional = true }

[dev-dependencies]
tempfile = "3"

[features]
default = ["ratatui"]
ratatui = ["dep:ratatui"]
```

`chronox/.gitignore`:

```
/target
Cargo.lock
```

- [ ] **Step 2: Minimal lib.rs that compiles in both feature modes**

`chronox/src/lib.rs`:

```rust
//! Parse Claude Code session logs into a timeline of file changes.
//!
//! The crate is split into a framework-agnostic **core** (parsing, timeline,
//! navigation, syntax tokenizing, config resolution) and an optional **ratatui
//! UI layer** behind the `ratatui` feature (bar/diff rendering). Enable the
//! `ratatui` feature (on by default) to use the rendering helpers.

pub mod config;
pub mod event;
pub mod extract;
pub mod nav;
pub mod syntax;
pub mod timeline;

#[cfg(feature = "ratatui")]
pub mod render;

pub use config::{ChronologyConfig, ChronologyOverride, ConfigSource, Side, WidthSpec};
pub use event::{ChangeDetail, ChangeEvent, ChangeSource, ChangeTool};
pub use nav::{NavAction, NavKey};
pub use syntax::{DiffLine, DiffMarker, LangSpec, Token, TokenKind, lang_for_path};
pub use timeline::Timeline;
```

This will not compile until the modules exist (Tasks 2–6). That is expected; do not build yet.

- [ ] **Step 3: Commit the scaffold**

```bash
git add -A
git commit -q -m "chore: scaffold chronox crate"
```

---

## Task 2: Port the data core (event, extract, timeline)

**Files (in the crate):**
- Create: `src/event.rs`, `src/extract.rs`, `src/timeline.rs`

This is a mechanical move of `src/activity/chronology.rs` plus the two inlined helpers. Split the single `chronology.rs` into three files by responsibility.

- [ ] **Step 1: Copy the type definitions into `event.rs`**

Copy `ChangeEvent`, `ChangeDetail`, `ChangeTool`, `ChangeSource` (and their derives/doc comments) verbatim from `src/activity/chronology.rs` into `src/event.rs`. Add `use std::path::PathBuf;` and `use serde::{...}` as the originals require. No logic changes.

- [ ] **Step 2: Copy parsing + helpers into `extract.rs`**

Copy into `src/extract.rs`:
- `DETAIL_MAX_CHARS`, `clip`, `extract_change_events`, `parse_file`, `load_full_change`, `resolve_line_in_file`, and the session-file discovery function — verbatim from `chronology.rs`.
- The two helpers from `src/activity/events.rs`: `encode_cwd` and `parse_iso8601_ms` — copied verbatim (plus their `#[cfg(test)]` tests).
- Replace any `use crate::activity::events::{encode_cwd, parse_iso8601_ms};` with crate-local references (they now live in this file).
- Replace `use` of the event types with `use crate::event::{ChangeDetail, ChangeEvent, ChangeSource, ChangeTool};`.

Bring the `chronology.rs` tests that cover these functions (`extract_tests`, `source_tests`, any `parse_file`/`load_full_change`/`resolve_line_in_file` tests) into `extract.rs` under `#[cfg(test)]`, fixing `use super::*;` and event-type imports.

- [ ] **Step 3: Copy the Timeline into `timeline.rs`**

Copy the `Timeline` type and its impl (`refresh`, `events`, the stat-based invalidation, any internal fields) into `src/timeline.rs`. Add `use crate::event::ChangeEvent;` and `use crate::extract::parse_file;` (and the session-discovery fn) as needed. Move its tests too.

- [ ] **Step 4: Verify the data core compiles and tests pass**

Run: `cargo test --no-default-features extract:: event:: timeline::`
Then: `cargo test --no-default-features`
Expected: PASS — the ported extract/source/timeline/helper tests are green; no ratatui in the tree.

(If `lib.rs` references modules not yet created — `config`, `nav`, `syntax` — temporarily comment their `pub mod`/`pub use` lines, OR implement Tasks 3–6 before the first full build. Recommended: comment them now, uncomment as each task lands. Track this so none stay commented.)

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -q -m "feat: port chronology data core (event, extract, timeline)"
```

---

## Task 3: Port the navigation state machine (nav)

**Files (in the crate):**
- Create: `src/nav.rs`

- [ ] **Step 1: Copy `chronology_nav.rs` verbatim into `nav.rs`**

Copy `NavKey`, `NavAction`, `nav`, `adjust_scroll`, `clamp_scroll` and all their `#[cfg(test)]` tests from `src/ui/chronology_nav.rs`. This module has no ratatui or `wsx` dependency, so the only edits are removing any `use crate::...` that no longer resolves (there should be none beyond `std`).

- [ ] **Step 2: Uncomment `pub mod nav;` / `pub use nav::...` in `lib.rs`**

- [ ] **Step 3: Verify**

Run: `cargo test --no-default-features nav::`
Expected: PASS — nav clamp/move, `clamp_scroll` bounds, `adjust_scroll` tests green.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -q -m "feat: port chronology navigation state machine"
```

---

## Task 4: Port syntax core with the neutral-token refactor

**Files (in the crate):**
- Create: `src/syntax.rs`

This is the one non-mechanical move: the tokenizer must stop emitting ratatui `Span`s and instead emit neutral `(String, TokenKind)` tokens. The ratatui mapping moves to `render.rs` (Task 5).

- [ ] **Step 1: Write the failing core test**

`src/syntax.rs` (test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn tokenize_priority_comment_string_number_keyword() {
        let spec = lang_for_path(Path::new("a.rs")).unwrap();
        // keyword + identifier + number + string
        let toks = tokenize_line("let x = 42 + \"hi\"", spec);
        // first run is the "let" keyword
        assert_eq!(toks[0], ("let".to_string(), TokenKind::Keyword));
        // a number run somewhere
        assert!(toks.iter().any(|(t, k)| t == "42" && *k == TokenKind::Number));
        // a string run including quotes
        assert!(toks.iter().any(|(t, k)| t == "\"hi\"" && *k == TokenKind::Str));
        // line comment swallows the rest of the line
        let c = tokenize_line("x // tail", spec);
        assert_eq!(c.last().unwrap(), &("// tail".to_string(), TokenKind::Comment));
    }

    #[test]
    fn change_detail_diff_gutter_and_marker() {
        let detail = ChangeDetail::Edit { old: "old".into(), new: "let y = 1".into() };
        let lines = change_detail_diff(&detail, 7, lang_for_path(Path::new("a.rs")));
        // removed line: blank gutter, Removed marker
        assert_eq!(lines[0].gutter, "     ");
        assert_eq!(lines[0].marker, DiffMarker::Removed);
        // added line: gutter "   7 ", Added marker, "let" tokenized as keyword
        assert_eq!(lines[1].gutter, "   7 ");
        assert_eq!(lines[1].marker, DiffMarker::Added);
        assert!(lines[1].code.iter().any(|(t, k)| t == "let" && *k == TokenKind::Keyword));
    }

    #[test]
    fn no_lang_is_single_default_run() {
        let detail = ChangeDetail::Write { head: "let y = 1".into() };
        let lines = change_detail_diff(&detail, 1, None);
        assert_eq!(lines[0].code, vec![("let y = 1".to_string(), TokenKind::Default)]);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --no-default-features syntax::`
Expected: FAIL — `tokenize_line`, `TokenKind`, `DiffLine`, `DiffMarker`, `change_detail_diff` not defined.

- [ ] **Step 3: Implement the core syntax module**

Copy the `LangSpec`, the four `static` language specs (`RUST`, `CLIKE`, `PYTHON`, `SHELL`), `lang_for_path`, and the tokenizer scaffolding (`take_while`, `take_string`) verbatim from `src/ui/syntax.rs`. Then add the neutral model and rewrite `highlight_code` as `tokenize_line`:

```rust
use crate::event::ChangeDetail;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind { Default, Keyword, Str, Number, Comment }

/// A run of source text and its highlight kind.
pub type Token = (String, TokenKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMarker { Added, Removed }

/// One display line of a change's diff: a fixed-width line-number gutter, an
/// add/remove marker, and the (optionally tokenized) code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub gutter: String,
    pub marker: DiffMarker,
    pub code: Vec<Token>,
}

fn push_default(out: &mut Vec<Token>, buf: &mut String) {
    if !buf.is_empty() {
        out.push((std::mem::take(buf), TokenKind::Default));
    }
}

/// Tokenize ONE line of code into (text, kind) runs by `spec`. Priority: line
/// comment (rest of line) > string > number > keyword/identifier > default.
pub fn tokenize_line(text: &str, spec: &LangSpec) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if spec.line_comment.iter().any(|c| rest.starts_with(c)) {
            push_default(&mut out, &mut buf);
            out.push((rest.to_string(), TokenKind::Comment));
            return out;
        }
        let ch = rest.chars().next().unwrap();
        if spec.string_delims.contains(&ch) {
            push_default(&mut out, &mut buf);
            let (tok, consumed) = take_string(rest, ch);
            out.push((tok, TokenKind::Str));
            i += consumed;
        } else if ch.is_ascii_digit() {
            push_default(&mut out, &mut buf);
            let (tok, consumed) = take_while(rest, |c| c.is_ascii_digit() || c == '.' || c == '_');
            out.push((tok, TokenKind::Number));
            i += consumed;
        } else if ch.is_alphabetic() || ch == '_' {
            let (tok, consumed) = take_while(rest, |c| c.is_alphanumeric() || c == '_');
            if spec.keywords.contains(&tok.as_str()) {
                push_default(&mut out, &mut buf);
                out.push((tok, TokenKind::Keyword));
            } else {
                buf.push_str(&tok);
            }
            i += consumed;
        } else {
            buf.push(ch);
            i += ch.len_utf8();
        }
    }
    push_default(&mut out, &mut buf);
    out
}

fn code_tokens(code: &str, lang: Option<&LangSpec>) -> Vec<Token> {
    match lang {
        Some(spec) => tokenize_line(code, spec),
        None => vec![(code.to_string(), TokenKind::Default)],
    }
}

/// Build the neutral diff model: removed (`old`) lines with a blank gutter and
/// `Removed` marker, then added (`new`/`head`) lines numbered from `base_line`
/// with an `Added` marker. No line cap — the modal scrolls.
pub fn change_detail_diff(
    detail: &ChangeDetail,
    base_line: u32,
    lang: Option<&LangSpec>,
) -> Vec<DiffLine> {
    let mut out = Vec::new();
    match detail {
        ChangeDetail::Edit { old, new } => {
            for l in old.lines() {
                out.push(DiffLine { gutter: "     ".to_string(), marker: DiffMarker::Removed, code: code_tokens(l, lang) });
            }
            for (k, l) in new.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                out.push(DiffLine { gutter: format!("{n:>4} "), marker: DiffMarker::Added, code: code_tokens(l, lang) });
            }
        }
        ChangeDetail::Write { head } => {
            for (k, l) in head.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                out.push(DiffLine { gutter: format!("{n:>4} "), marker: DiffMarker::Added, code: code_tokens(l, lang) });
            }
        }
        ChangeDetail::None => {}
    }
    out
}
```

Notes:
- `LangSpec`, the language statics, `lang_for_path`, `take_while`, `take_string` are copied unchanged from the original `syntax.rs`. Delete the original ratatui `*_style()` fns, `flush`, `highlight_code`, `code_spans`, `change_detail_lines_styled`, and `clip_line_to_width` from this core file — they move to `render.rs` (Task 5).
- The gutter strings (`"     "` 5 spaces; `"{n:>4} "` = 4-wide right-aligned + space) and marker semantics exactly match the current `change_detail_lines_styled`.

- [ ] **Step 4: Uncomment `pub mod syntax;` / `pub use syntax::...` in `lib.rs`**

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --no-default-features syntax::` (the three tests pass), then `cargo test --no-default-features` (whole core green).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -q -m "feat: port syntax core as neutral token model (no ratatui)"
```

---

## Task 5: Add the ratatui render layer (feature-gated)

**Files (in the crate):**
- Create: `src/render.rs`

This is where the neutral tokens/diff become ratatui `Line`/`Span`, preserving the exact colours the app ships today. It also holds `entry_lines` (the bar row renderer) since that is inherently ratatui.

- [ ] **Step 1: Write the failing feature-gated test**

`src/render.rs` (test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ChangeDetail;
    use crate::syntax::lang_for_path;
    use ratatui::style::{Color, Modifier};
    use std::path::Path;

    #[test]
    fn styled_lines_preserve_colours_and_gutter() {
        let detail = ChangeDetail::Edit { old: "old".into(), new: "let y = 1".into() };
        let lines = change_detail_lines_styled(&detail, 7, lang_for_path(Path::new("a.rs")));
        // removed line: dim 5-space gutter, red "- " marker
        assert_eq!(lines[0].spans[0].content.as_ref(), "     ");
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[0].spans[1].content.as_ref(), "- ");
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Red));
        // added line: gutter "   7 ", green "+ ", "let" highlighted magenta
        assert_eq!(lines[1].spans[0].content.as_ref(), "   7 ");
        assert_eq!(lines[1].spans[1].content.as_ref(), "+ ");
        assert_eq!(lines[1].spans[1].style.fg, Some(Color::Green));
        assert!(lines[1].spans.iter().any(|s| s.content.as_ref() == "let" && s.style.fg == Some(Color::Magenta)));
    }

    #[test]
    fn no_lang_is_plain_code_span() {
        let detail = ChangeDetail::Write { head: "let y = 1".into() };
        let lines = change_detail_lines_styled(&detail, 1, None);
        assert_eq!(lines[0].spans[2].content.as_ref(), "let y = 1");
        assert_eq!(lines[0].spans[2].style.fg, None);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --features ratatui render::`
Expected: FAIL — `change_detail_lines_styled` not defined in `render`.

- [ ] **Step 3: Implement the render layer**

`src/render.rs`:

```rust
//! ratatui rendering for the chronology UI. Maps the core's neutral
//! `TokenKind`/`DiffLine` model to styled ratatui `Line`/`Span`, and renders
//! bar rows. Only compiled with the `ratatui` feature.

use crate::event::ChangeEvent;
use crate::syntax::{DiffLine, DiffMarker, LangSpec, Token, TokenKind, change_detail_diff};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;

fn style_for(kind: TokenKind) -> Style {
    match kind {
        TokenKind::Keyword => Style::default().fg(Color::Magenta),
        TokenKind::Str => Style::default().fg(Color::Yellow),
        TokenKind::Comment => Style::default().fg(Color::DarkGray),
        TokenKind::Number => Style::default().fg(Color::Cyan),
        TokenKind::Default => Style::default(),
    }
}

fn token_spans(code: &[Token]) -> Vec<Span<'static>> {
    code.iter().map(|(t, k)| Span::styled(t.clone(), style_for(*k))).collect()
}

fn diff_line_to_ratatui(dl: &DiffLine) -> Line<'static> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let (marker, marker_style) = match dl.marker {
        DiffMarker::Added => ("+ ", Style::default().fg(Color::Green)),
        DiffMarker::Removed => ("- ", Style::default().fg(Color::Red)),
    };
    let mut spans = vec![
        Span::styled(dl.gutter.clone(), dim),
        Span::styled(marker.to_string(), marker_style),
    ];
    spans.extend(token_spans(&dl.code));
    Line::from(spans)
}

/// Build the modal's styled diff lines from a change. Same colours/gutter as the
/// in-`wsx` implementation it replaces.
pub fn change_detail_lines_styled(
    detail: &crate::event::ChangeDetail,
    base_line: u32,
    lang: Option<&LangSpec>,
) -> Vec<Line<'static>> {
    change_detail_diff(detail, base_line, lang).iter().map(diff_line_to_ratatui).collect()
}

/// Truncate a styled `Line` to `width` display columns (char-based), preserving
/// span styles; the boundary span is trimmed.
pub fn clip_line_to_width(line: &Line<'static>, width: usize) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in &line.spans {
        if used >= width {
            break;
        }
        let remaining = width - used;
        let cnt = span.content.chars().count();
        if cnt <= remaining {
            out.push(span.clone());
            used += cnt;
        } else {
            let truncated: String = span.content.chars().take(remaining).collect();
            out.push(Span::styled(truncated, span.style));
            break;
        }
    }
    Line::from(out)
}
```

Then move `entry_lines` and its display helpers (`relative_display`, `hhmm`, `abbreviate_path`, `should_auto_hide`) from `src/ui/chronology_bar.rs` into `render.rs`, verbatim, fixing imports (`use crate::event::ChangeEvent;`, `Path`). Bring their tests (`relative_path_*`, `auto_hide_*`, `abbreviate_*`, the `entry_lines` header/selected tests) into this module under `#[cfg(test)]`. `clip_line_to_width` is moved here verbatim from the old `syntax.rs`.

Add the public re-exports for these to `lib.rs` under the feature gate:

```rust
#[cfg(feature = "ratatui")]
pub use render::{change_detail_lines_styled, clip_line_to_width, entry_lines};
```

- [ ] **Step 4: Verify all three feature modes**

Run, all expected PASS:
```
cargo test --no-default-features   # core only — no render module compiled
cargo test --features ratatui      # render tests + core
cargo test --all-features
cargo build --no-default-features   # core compiles with zero ratatui in tree
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -q -m "feat: ratatui render layer (feature-gated) over neutral tokens"
```

---

## Task 6: Port config onto a `ConfigSource` trait

**Files (in the crate):**
- Create: `src/config.rs`

- [ ] **Step 1: Write the failing test**

`src/config.rs` (test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct MemSource { global: Option<String>, repo: Option<String> }
    impl ConfigSource for MemSource {
        fn global_json(&self) -> Option<String> { self.global.clone() }
        fn repo_override_json(&self) -> Option<String> { self.repo.clone() }
    }

    #[test]
    fn resolve_applies_repo_override_with_global_unset() {
        let src = MemSource { global: None, repo: Some(r#"{"side":"left"}"#.into()) };
        let cfg = resolve(&src);
        assert_eq!(cfg.side, Side::Left);
        assert_eq!(cfg.visible, ChronologyConfig::default().visible);
        assert_eq!(cfg.width, ChronologyConfig::default().width);
    }

    #[test]
    fn default_is_visible_right_sane_width() {
        let c = ChronologyConfig::default();
        assert!(c.visible);
        assert_eq!(c.side, Side::Right);
        assert_eq!(c.width.percent, 32);
        assert!(c.width.min_cols <= c.width.max_cols);
    }

    #[test]
    fn override_merges_per_field() {
        let base = ChronologyConfig::default();
        let ovr = ChronologyOverride { visible: Some(false), side: Some(Side::Left), width: None };
        let merged = base.with_override(&ovr);
        assert!(!merged.visible);
        assert_eq!(merged.side, Side::Left);
        assert_eq!(merged.width.percent, 32);
    }

    #[test]
    fn sanitize_clamps_and_swaps() {
        let mut c = ChronologyConfig::default();
        c.width.percent = 99; c.width.min_cols = 80; c.width.max_cols = 10;
        c.sanitize();
        assert!(c.width.percent <= 80);
        assert!(c.width.min_cols <= c.width.max_cols);
    }

    #[test]
    fn resolved_width_clamps_to_min_and_max() {
        let mut c = ChronologyConfig::default();
        c.width.percent = 50; c.width.min_cols = 20; c.width.max_cols = 30;
        assert_eq!(c.resolved_width(200), 30);
        assert_eq!(c.resolved_width(20), 20);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --no-default-features config::`
Expected: FAIL — `config` types / `ConfigSource` / `resolve` not defined.

- [ ] **Step 3: Implement**

Copy `Side`, `WidthSpec`, `ChronologyConfig`, `ChronologyOverride` and their `Default`/`with_override`/`sanitize`/`resolved_width` impls verbatim from `src/config/chronology.rs` into `src/config.rs`. Then replace the `Store`/`Repo` resolvers with the trait-based pair:

```rust
use serde::{Deserialize, Serialize};

/// Supplies the two raw JSON blobs the resolver merges (global settings +
/// per-repo override). Consumers implement this over their own storage.
pub trait ConfigSource {
    /// The global `chronology_config` JSON, if set.
    fn global_json(&self) -> Option<String>;
    /// The per-repo `chronology_config` override JSON, if set.
    fn repo_override_json(&self) -> Option<String>;
}

/// Resolve the global config only (no repo override). Defaults on missing key
/// or parse failure (a warning is logged via the `log` facade).
pub fn resolve_global_only(src: &impl ConfigSource) -> ChronologyConfig {
    let mut cfg = match src.global_json() {
        Some(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            log::warn!("chronology_config: global parse failed ({e}); using defaults");
            ChronologyConfig::default()
        }),
        None => ChronologyConfig::default(),
    };
    cfg.sanitize();
    cfg
}

/// Resolve global config with the per-repo override applied (repo wins per-field).
pub fn resolve(src: &impl ConfigSource) -> ChronologyConfig {
    let mut cfg = resolve_global_only(src);
    if let Some(raw) = src.repo_override_json() {
        match serde_json::from_str::<ChronologyOverride>(&raw) {
            Ok(ovr) => cfg = cfg.with_override(&ovr),
            Err(e) => log::warn!("chronology_config: repo override parse failed ({e}); ignoring"),
        }
    }
    cfg.sanitize();
    cfg
}
```

- [ ] **Step 4: Uncomment `pub mod config;` / `pub use config::...` in `lib.rs`** (it should already list `ConfigSource`).

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --no-default-features config::` (5 pass), then full `cargo test --all-features`, `cargo build --no-default-features`, `cargo clippy --all-targets --all-features`, `cargo fmt --check`.

- [ ] **Step 6: Commit and record the rev**

```bash
git add -A
git commit -q -m "feat: config resolution over a ConfigSource trait"
git rev-parse HEAD   # note this sha — wsx pins it in Task 10
```

---

## Task 7: Crate finalization & sanity build

**Files (in the crate):** `src/lib.rs`

- [ ] **Step 1: Confirm no commented-out module lines remain in `lib.rs`** (from Task 2's staging). All of `config`, `event`, `extract`, `nav`, `syntax`, `timeline`, and the feature-gated `render` are declared and re-exported.

- [ ] **Step 2: Full verification matrix**

Run, all expected PASS / clean:
```
cargo build --no-default-features
cargo build --all-features
cargo test --no-default-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

- [ ] **Step 3: Commit any fmt/clippy fixups**

```bash
git add -A
git commit -q -m "chore: lib re-exports and lint pass" || echo "nothing to commit"
```

---

## Task 8: Wire `wsx` to the crate via a path override + config adapter

**Files (in `wsx`):**
- Modify: `Cargo.toml`
- Create: `src/config/chronology_source.rs`
- Modify: `src/config/mod.rs`

Work back in the `wsx` repo for the rest of the plan.

- [ ] **Step 1: Add the dependency as a PATH override (iteration only)**

In `wsx/Cargo.toml` `[dependencies]`:

```toml
chronox = { path = "/home/eben/chronox", features = ["ratatui"] }
```

(Replaced by a pinned `git`+`rev` in Task 10. Do not commit this path form as final.)

- [ ] **Step 2: Write the failing adapter test**

`wsx/src/config/chronology_source.rs`:

```rust
//! Adapter implementing the crate's `ConfigSource` over wsx's `Store`/`Repo`.

use crate::data::store::{Repo, Store};
use chronox::ConfigSource;

/// Borrows the global store and an optional repo to feed the chronology config
/// resolver.
pub struct StoreConfigSource<'a> {
    pub store: &'a Store,
    pub repo: Option<&'a Repo>,
}

impl ConfigSource for StoreConfigSource<'_> {
    fn global_json(&self) -> Option<String> {
        self.store.get_setting("chronology_config").ok().flatten()
    }
    fn repo_override_json(&self) -> Option<String> {
        self.repo.and_then(|r| r.chronology_config.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronox::{Side, resolve};

    #[test]
    fn adapter_resolves_repo_override() {
        let store = Store::open_in_memory().unwrap();
        // build a Repo with chronology_config set to flip side left
        let mut repo = crate::data::store::Repo::default();
        repo.chronology_config = Some(r#"{"side":"left"}"#.to_string());
        let src = StoreConfigSource { store: &store, repo: Some(&repo) };
        assert_eq!(resolve(&src).side, Side::Left);
    }
}
```

(If `Repo` has no `Default`, construct it with the full literal as the existing `config/chronology.rs` test does — copy that `test_repo` helper.)

- [ ] **Step 3: Register the module**

In `wsx/src/config/mod.rs`: add `pub mod chronology_source;` and remove `pub mod chronology;` (the old module is deleted in Task 9; if removing it now breaks the build because callers still reference `config::chronology::resolve`, keep it until Task 9 and do the swap there — note which order you take).

- [ ] **Step 4: Run to verify**

Run: `cargo test --lib chronology_source`
Expected: PASS (the crate resolves via the adapter). The wider build may still fail until Task 9 swaps call sites — that is fine; this task only proves the adapter + dependency link work.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/config/chronology_source.rs src/config/mod.rs
git commit -q -m "feat: depend on chronox + ConfigSource adapter"
```

---

## Task 9: Delete moved modules and re-point `wsx` imports

**Files (in `wsx`):**
- Delete: `src/activity/chronology.rs`, `src/ui/chronology_nav.rs`, `src/ui/chronology_bar.rs`, `src/ui/syntax.rs`, `src/config/chronology.rs`
- Modify: `src/activity/mod.rs`, `src/ui/mod.rs`, `src/config/mod.rs`, `src/app.rs`, `src/app/input.rs`, `src/app/render.rs`, `src/ui/attached.rs`, `src/ui/modal.rs`

This is one cohesive cut — the tree won't compile mid-way. Do it as a single task and lean on the compiler to find every reference.

- [ ] **Step 1: Delete the five moved modules and drop their `mod` lines**

```bash
git rm src/activity/chronology.rs src/ui/chronology_nav.rs src/ui/chronology_bar.rs src/ui/syntax.rs src/config/chronology.rs
```
Remove the corresponding `mod chronology;` / `mod chronology_nav;` / `mod chronology_bar;` / `mod syntax;` lines from `src/activity/mod.rs`, `src/ui/mod.rs`, `src/config/mod.rs`.

- [ ] **Step 2: Re-point every import to the crate**

Build and let the compiler enumerate the breaks: `cargo build 2>&1 | head -50`. Fix each by replacing the old path with the crate path. Expected substitutions:
- `crate::activity::chronology::{ChangeEvent, ChangeDetail, ChangeTool, ChangeSource, Timeline, extract_change_events, parse_file, load_full_change, resolve_line_in_file, …}` → `chronox::{…}` (same item names).
- `crate::ui::chronology_nav::{nav, NavKey, NavAction, adjust_scroll, clamp_scroll}` → `chronox::nav::{…}` (or the `NavKey`/`NavAction` re-exports at crate root).
- `crate::ui::chronology_bar::{entry_lines, relative_display, hhmm, abbreviate_path, should_auto_hide}` → `chronox::{entry_lines, …}` / `chronox::render::{…}` for the non-re-exported helpers.
- `crate::ui::syntax::{change_detail_lines_styled, clip_line_to_width, lang_for_path, LangSpec}` → `chronox::{change_detail_lines_styled, clip_line_to_width, lang_for_path, LangSpec}`.
- `crate::config::chronology::{resolve, resolve_global_only, ChronologyConfig, Side, …}` → call sites now build a `StoreConfigSource { store, repo }` and call `chronox::resolve(&src)` / `resolve_global_only(&src)`. Config *types* → `chronox::{ChronologyConfig, Side, WidthSpec, …}`.

- [ ] **Step 3: Switch the App cache type**

In `src/app.rs`: `chronology: HashMap<WorkspaceId, chronox::Timeline>` and any `Timeline::default()`/method calls now resolve to the crate type (same API: `refresh`, `events`). `refresh_chronology` uses `chronox::parse_file` / the crate's session-discovery fn exactly as before.

- [ ] **Step 4: Fix config resolver call sites**

Wherever `wsx` previously called `config::chronology::resolve(repo, store)` (e.g. in `attached.rs`/`render.rs` when building `ChronologyDraw`), replace with:
```rust
let src = crate::config::chronology_source::StoreConfigSource { store: &app.store, repo: repo_opt };
let cfg = chronox::resolve(&src);
```
Match the actual variable names/borrows at each site.

- [ ] **Step 5: Verify the full build and tests**

Run, all expected PASS / clean:
```
cargo build 2>&1 | tail -5      # zero errors, zero warnings
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
(Some chronology unit tests no longer live in `wsx` — they moved to the crate. The `wsx` suite now covers the integration layer + the adapter test. Confirm the count dropped by roughly the moved tests and nothing else regressed.)

- [ ] **Step 6: Manual TUI pass**

Build and run `wsx`; attach to a workspace with recent Claude activity:
- Focus the chronology bar (`Ctrl-x`+arrow), navigate with `j`/`k`/`g`/`G`.
- `Enter` (and click) opens the detail modal with the full diff + line-number gutter.
- Syntax colours present (keywords magenta, strings yellow, comments gray, numbers cyan); `+` green / `-` red; gutter dim.
- Scroll with arrows/`j`/`k`/`PgUp`/`PgDn`/`g`/`G`/wheel; `e` opens the editor at the change line; `Esc`/click-outside closes.
- Confirm the bar shows/hides per the `side`/`width`/`visible` config (set a per-repo `chronology_config` override and confirm it still applies through the adapter).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -q -m "refactor: consume chronox crate; remove in-tree modules"
```

---

## Task 10: Pin `wsx` to a git rev

**Files (in `wsx`):** `Cargo.toml`

- [ ] **Step 1: Push the crate to its remote**

In the crate repo: create the remote (e.g. `gh repo create chronox --private --source . --remote origin`), `git push -u origin main`. Capture the pushed commit sha.

- [ ] **Step 2: Replace the path dependency with a pinned git rev**

In `wsx/Cargo.toml`:

```toml
chronox = { git = "https://github.com/<owner>/chronox", rev = "<sha-from-Task-6/7>", features = ["ratatui"] }
```

- [ ] **Step 3: Verify against the pinned rev**

Run: `cargo update -p chronox` then `cargo build` then `cargo test`.
Expected: resolves from git, builds, tests pass — identical behaviour to the path build.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -q -m "build: pin chronox to a git rev"
```

- [ ] **Step 5: Open the wsx PR**

Per the repo rule (never commit to `main` directly), this work is on the `extract-chronology-library` branch. Push and open a PR. (The crate repo is independent; it has its own history.)

---

## Task 11 (OPTIONAL, deferred): crates.io publish polish

Only do this if/when the decision to publish is taken. Not part of the extraction.

- [ ] README with usage + the core/ratatui feature split documented.
- [ ] Runnable example under `examples/` (parse a sample session log → print a timeline).
- [ ] CI workflow running the three feature-mode test matrices + clippy + fmt.
- [ ] Confirm `LICENSE` files match the `Cargo.toml` `license` field (MIT/Apache dual).
- [ ] `cargo publish --dry-run`; review the packaged file list; document the JSONL-format-tracking maintenance commitment in the README.

---

## Self-Review (completed during planning)

**Spec coverage:** new crate scaffold w/ feature flags (T1) ✓; data core move event/extract/timeline incl. inlined `encode_cwd`/`parse_iso8601_ms` — Seam 3 (T2) ✓; nav move (T3) ✓; Seam 1 syntax neutral-token refactor (T4) + ratatui render layer feature-gated (T5) ✓; Seam 2 `ConfigSource` trait (T6) + wsx adapter (T8) ✓; crate finalization & 3-mode verify (T7) ✓; wsx delete + re-point imports + App cache type + config call sites (T9) ✓; git-rev pinning + PR (T10) ✓; deferred publish (T11) ✓. ratatui-version lockstep enforced via `0.29` pin in T1 + T8. Manual TUI regression (T9 step 6) matches the spec's testing section.

**Placeholder scan:** `<sha>`, `<owner>` are real parameters captured at T6/T10 (the repo/sha don't exist until then), not deferred work. The "copy verbatim from file X" instructions for mechanical moves name exact symbols + import substitutions and carry the existing tests — complete, not hand-waved. Full code is given for every shape-changing piece (Cargo.toml, lib.rs, syntax core, render layer, config trait, adapter).

**Type consistency:** `TokenKind{Default,Keyword,Str,Number,Comment}`, `Token=(String,TokenKind)`, `DiffLine{gutter,marker,code}`, `DiffMarker{Added,Removed}`, `tokenize_line`, `change_detail_diff` (core) → `change_detail_lines_styled`/`clip_line_to_width`/`entry_lines` (render, feature-gated); `ConfigSource{global_json,repo_override_json}` + `resolve`/`resolve_global_only` (crate) consumed via `StoreConfigSource{store,repo}` (wsx); `ChangeEvent/ChangeDetail/ChangeTool/ChangeSource/Timeline` names unchanged across the move so wsx import substitution is name-preserving. Colours/gutter scheme in T5 match the current `change_detail_lines_styled` exactly.
