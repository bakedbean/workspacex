# Chronology Modal Syntax Highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Basic, dependency-free syntax highlighting + diff-tinted markers for the code shown in the chronology detail modal.

**Architecture:** A new pure `src/ui/syntax.rs` — a generic per-language tokenizer (`LangSpec` + `highlight_code`), `lang_for_path`, a styled diff-line builder (`change_detail_lines_styled`), and `clip_line_to_width`. The modal stores pre-highlighted `Vec<Line<'static>>` (built on open); the renderer clips styled spans to width.

**Tech Stack:** Rust, `ratatui` (`Span`/`Line`/`Style`/`Color`). No new deps. `#[cfg(test)]` unit tests.

**Builds on (verified current code):**
- `src/ui/modal.rs`: `Modal::ChangeDetail { title: String, lines: Vec<String>, scroll: usize, worktree: PathBuf, file: PathBuf, line: u32 }` (+ an early-return guard and an `unreachable!` arm both matching `Modal::ChangeDetail { .. }`).
- `src/app/render.rs` `render_change_detail_modal(f, area, title, lines: &[String], scroll, theme)`: visible lines built as `Line::from(Span::raw(l.chars().take(inner.width).collect()))`; uses `clamp_scroll`; footer with `n-m/total`.
- `src/app/input.rs` `open_change_modal(app, idx)`: builds `detail` (via `load_full_change` fallback), `line` (`resolve_line_in_file`), `lines = crate::ui::chronology_bar::change_detail_lines(&detail, line)`, `title`, and sets `Modal::ChangeDetail{…}`. Modal input handler clones the modal and reads `lines.len()`.
- `src/ui/chronology_bar.rs`: `pub fn change_detail_lines(detail, base_line) -> Vec<String>` (added/removed gutter format `"{n:>4} + {l}"` / `"     - {l}"`) + 3 tests (`change_detail_lines_*`). Only `open_change_modal` calls it.

---

## File Structure

- `src/ui/syntax.rs` (new) — `LangSpec`, lang statics, `lang_for_path`, `highlight_code`, `change_detail_lines_styled`, `clip_line_to_width`, palette, tests.
- `src/ui/mod.rs` — `pub mod syntax;`.
- `src/ui/modal.rs` — `ChangeDetail.lines` type → `Vec<ratatui::text::Line<'static>>`.
- `src/app/render.rs` — render styled lines via `clip_line_to_width`.
- `src/app/input.rs` — `open_change_modal` uses `syntax::change_detail_lines_styled` + `lang_for_path`.
- `src/ui/chronology_bar.rs` — remove `change_detail_lines` (+ its 3 tests).
- `README.md`.

---

## Task 1: `syntax.rs` — `LangSpec`, `lang_for_path`, `highlight_code`

**Files:** Create `src/ui/syntax.rs`; modify `src/ui/mod.rs`.

- [ ] **Step 1: Create the module with tests-first**

Create `src/ui/syntax.rs`:

```rust
//! Basic, dependency-free syntax highlighting for the chronology detail modal.
//! A single generic tokenizer driven by a per-language `LangSpec`. Per-line,
//! no cross-line state — "basic" fidelity for a glanceable diff.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;

/// Minimal language description driving the generic tokenizer.
pub struct LangSpec {
    pub keywords: &'static [&'static str],
    pub line_comment: &'static [&'static str],
    pub string_delims: &'static [char],
}

static RUST: LangSpec = LangSpec {
    keywords: &[
        "fn", "let", "mut", "pub", "use", "struct", "enum", "impl", "trait", "for", "in", "if",
        "else", "match", "while", "loop", "return", "self", "Self", "mod", "const", "static",
        "move", "ref", "as", "where", "async", "await", "dyn", "crate", "super", "type", "unsafe",
        "break", "continue", "true", "false",
    ],
    line_comment: &["//"],
    string_delims: &['"'],
};

static CLIKE: LangSpec = LangSpec {
    keywords: &[
        "if", "else", "for", "while", "switch", "case", "break", "continue", "return", "struct",
        "class", "const", "static", "void", "int", "char", "bool", "new", "delete", "public",
        "private", "protected", "function", "var", "let", "import", "export", "from", "default",
        "true", "false", "null",
    ],
    line_comment: &["//"],
    string_delims: &['"', '\''],
};

static PYTHON: LangSpec = LangSpec {
    keywords: &[
        "def", "class", "return", "if", "elif", "else", "for", "while", "import", "from", "as",
        "with", "try", "except", "finally", "raise", "lambda", "None", "True", "False", "and",
        "or", "not", "in", "is", "pass", "yield", "global", "nonlocal",
    ],
    line_comment: &["#"],
    string_delims: &['"', '\''],
};

static SHELL: LangSpec = LangSpec {
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case", "esac",
        "function", "return", "export", "local",
    ],
    line_comment: &["#"],
    string_delims: &['"', '\''],
};

/// Pick a `LangSpec` from a path's extension; `None` → no highlighting.
pub fn lang_for_path(path: &Path) -> Option<&'static LangSpec> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    match ext {
        "rs" => Some(&RUST),
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "js" | "jsx" | "ts" | "tsx" | "go"
        | "java" | "cs" | "json" => Some(&CLIKE),
        "py" => Some(&PYTHON),
        "sh" | "bash" | "zsh" => Some(&SHELL),
        _ => None,
    }
}

fn kw_style() -> Style { Style::default().fg(Color::Magenta) }
fn str_style() -> Style { Style::default().fg(Color::Yellow) }
fn comment_style() -> Style { Style::default().fg(Color::DarkGray) }
fn num_style() -> Style { Style::default().fg(Color::Cyan) }

fn flush(spans: &mut Vec<Span<'static>>, default: &mut String) {
    if !default.is_empty() {
        spans.push(Span::raw(std::mem::take(default)));
    }
}

fn take_while(rest: &str, pred: impl Fn(char) -> bool) -> (String, usize) {
    let mut tok = String::new();
    let mut bytes = 0;
    for c in rest.chars() {
        if pred(c) {
            tok.push(c);
            bytes += c.len_utf8();
        } else {
            break;
        }
    }
    (tok, bytes)
}

fn take_string(rest: &str, delim: char) -> (String, usize) {
    let mut tok = String::new();
    let mut bytes = 0;
    let mut chars = rest.chars();
    let open = chars.next().unwrap();
    tok.push(open);
    bytes += open.len_utf8();
    let mut escaped = false;
    for c in chars {
        tok.push(c);
        bytes += c.len_utf8();
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == delim {
            break;
        }
    }
    (tok, bytes)
}

/// Tokenize ONE line of code into styled spans by `spec`. Priority: line
/// comment (rest of line) > string > number > keyword/identifier > default.
pub fn highlight_code(text: &str, spec: &LangSpec) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut default = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(cm) = spec.line_comment.iter().find(|c| rest.starts_with(**c)) {
            let _ = cm;
            flush(&mut spans, &mut default);
            spans.push(Span::styled(rest.to_string(), comment_style()));
            return spans;
        }
        let ch = rest.chars().next().unwrap();
        if spec.string_delims.contains(&ch) {
            flush(&mut spans, &mut default);
            let (tok, consumed) = take_string(rest, ch);
            spans.push(Span::styled(tok, str_style()));
            i += consumed;
        } else if ch.is_ascii_digit() {
            flush(&mut spans, &mut default);
            let (tok, consumed) = take_while(rest, |c| c.is_ascii_digit() || c == '.' || c == '_');
            spans.push(Span::styled(tok, num_style()));
            i += consumed;
        } else if ch.is_alphabetic() || ch == '_' {
            let (tok, consumed) = take_while(rest, |c| c.is_alphanumeric() || c == '_');
            if spec.keywords.contains(&tok.as_str()) {
                flush(&mut spans, &mut default);
                spans.push(Span::styled(tok, kw_style()));
            } else {
                default.push_str(&tok);
            }
            i += consumed;
        } else {
            default.push(ch);
            i += ch.len_utf8();
        }
    }
    flush(&mut spans, &mut default);
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_for_path_maps_extensions() {
        assert!(lang_for_path(Path::new("a.rs")).is_some());
        assert!(lang_for_path(Path::new("a.py")).is_some());
        assert!(lang_for_path(Path::new("a.c")).is_some());
        assert!(lang_for_path(Path::new("a.js")).is_some());
        assert!(lang_for_path(Path::new("a.sh")).is_some());
        assert!(lang_for_path(Path::new("a.txt")).is_none());
        assert!(lang_for_path(Path::new("noext")).is_none());
    }

    fn texts(spans: &[Span<'static>]) -> Vec<(String, Option<Color>)> {
        spans.iter().map(|s| (s.content.to_string(), s.style.fg)).collect()
    }

    #[test]
    fn highlight_rust_keyword_string_comment_number() {
        let spans = highlight_code(r#"let x = "hi"; // c"#, &RUST);
        let t = texts(&spans);
        assert!(t.iter().any(|(s, c)| s == "let" && *c == Some(Color::Magenta)), "{t:?}");
        assert!(t.iter().any(|(s, c)| s == "\"hi\"" && *c == Some(Color::Yellow)), "{t:?}");
        assert!(t.iter().any(|(s, c)| s == "// c" && *c == Some(Color::DarkGray)), "{t:?}");

        let nums = highlight_code("x = 42", &RUST);
        assert!(texts(&nums).iter().any(|(s, c)| s == "42" && *c == Some(Color::Cyan)));
    }

    #[test]
    fn highlight_string_keeps_escaped_quote_in_one_span() {
        let spans = highlight_code(r#""a\"b""#, &RUST);
        let t = texts(&spans);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].0, r#""a\"b""#);
        assert_eq!(t[0].1, Some(Color::Yellow));
    }

    #[test]
    fn non_keyword_identifier_is_default() {
        let spans = highlight_code("foobar", &RUST);
        let t = texts(&spans);
        assert_eq!(t, vec![("foobar".to_string(), None)]);
    }
}
```

In `src/ui/mod.rs` add `pub mod syntax;`.

- [ ] **Step 2: Run to verify (red→green)**

Run: `cargo test --lib syntax` → the 4 tests pass once the module compiles. (No separate red phase needed; if any fail, the tokenizer is wrong — fix it.)

- [ ] **Step 3: Build**

Run: `cargo build` — clean. (Unused warnings for `change_detail_lines_styled`/`clip_line_to_width` don't exist yet; they're added next task. `highlight_code`/`lang_for_path` are `pub`, consumed next task — no dead-code warning.)

- [ ] **Step 4: Commit**

```bash
git add src/ui/syntax.rs src/ui/mod.rs
git commit -m "feat(chronology): basic per-language syntax tokenizer (syntax.rs)"
```

---

## Task 2: styled diff-line builder + `clip_line_to_width`

**Files:** Modify `src/ui/syntax.rs`.

- [ ] **Step 1: Failing tests**

Add to `src/ui/syntax.rs` tests (it needs `ChangeDetail`):

```rust
    use crate::activity::chronology::ChangeDetail;

    fn line_text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn styled_lines_marker_colors_and_gutter() {
        let detail = ChangeDetail::Edit { old: "old".into(), new: "let y = 1".into() };
        let lines = change_detail_lines_styled(&detail, 7, lang_for_path(Path::new("a.rs")));
        // removed line: dim gutter (5 spaces), red "- " marker
        assert_eq!(lines[0].spans[0].content.as_ref(), "     ");
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[0].spans[1].content.as_ref(), "- ");
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Red));
        // added line: gutter "   7 ", green "+ ", code highlighted (let = magenta)
        assert_eq!(lines[1].spans[0].content.as_ref(), "   7 ");
        assert_eq!(lines[1].spans[1].content.as_ref(), "+ ");
        assert_eq!(lines[1].spans[1].style.fg, Some(Color::Green));
        assert!(lines[1].spans.iter().any(|s| s.content.as_ref() == "let" && s.style.fg == Some(Color::Magenta)));
    }

    #[test]
    fn styled_lines_plain_when_no_lang() {
        let detail = ChangeDetail::Write { head: "let y = 1".into() };
        let lines = change_detail_lines_styled(&detail, 1, None);
        // code is a single default span (no highlighting)
        assert_eq!(lines[0].spans[2].content.as_ref(), "let y = 1");
        assert_eq!(lines[0].spans[2].style.fg, None);
    }

    #[test]
    fn clip_line_preserves_styles_and_truncates() {
        let detail = ChangeDetail::Write { head: "abcdefgh".into() };
        let line = &change_detail_lines_styled(&detail, 1, None)[0]; // "   1 + abcdefgh"
        let clipped = clip_line_to_width(line, 7);
        assert_eq!(line_text(&clipped), "   1 + ");
        assert_eq!(clip_line_to_width(line, 0).spans.len(), 0);
        let wide = clip_line_to_width(line, 999);
        assert_eq!(line_text(&wide), "   1 + abcdefgh");
    }
```

- [ ] **Step 2: Verify failure**

Run: `cargo test --lib syntax` → FAIL (`change_detail_lines_styled` / `clip_line_to_width` undefined).

- [ ] **Step 3: Implement**

Add to `src/ui/syntax.rs` (non-test), with `use crate::activity::chronology::ChangeDetail;` at the top:

```rust
fn code_spans(code: &str, lang: Option<&LangSpec>) -> Vec<Span<'static>> {
    match lang {
        Some(spec) => highlight_code(code, spec),
        None => vec![Span::raw(code.to_string())],
    }
}

/// Build the modal's styled diff lines: dim 4-col gutter, green `+` / red `-`
/// marker, then highlighted code (or a plain span when `lang` is None). Added
/// lines numbered from `base_line`; removed lines blank gutter. No line cap.
pub fn change_detail_lines_styled(
    detail: &ChangeDetail,
    base_line: u32,
    lang: Option<&LangSpec>,
) -> Vec<Line<'static>> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let add = Style::default().fg(Color::Green);
    let del = Style::default().fg(Color::Red);
    let mut out = Vec::new();
    let mut push = |gutter: String, marker_style: Style, marker: &str, code: &str, out: &mut Vec<Line<'static>>| {
        let mut spans = vec![
            Span::styled(gutter, dim),
            Span::styled(marker.to_string(), marker_style),
        ];
        spans.extend(code_spans(code, lang));
        out.push(Line::from(spans));
    };
    match detail {
        ChangeDetail::Edit { old, new } => {
            for l in old.lines() {
                push("     ".to_string(), del, "- ", l, &mut out);
            }
            for (k, l) in new.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                push(format!("{n:>4} "), add, "+ ", l, &mut out);
            }
        }
        ChangeDetail::Write { head } => {
            for (k, l) in head.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                push(format!("{n:>4} "), add, "+ ", l, &mut out);
            }
        }
        ChangeDetail::None => {}
    }
    out
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

(The `push` closure captures `dim`/`add`/`del`/`lang` — all `Copy` — and takes `out` as a param to avoid a double mutable borrow.)

- [ ] **Step 4: Verify pass**

Run: `cargo test --lib syntax` (all pass), `cargo build` (zero warnings), `cargo fmt` + `cargo fmt --check`.

- [ ] **Step 5: Commit**

```bash
git add src/ui/syntax.rs
git commit -m "feat(chronology): styled diff-line builder + clip_line_to_width"
```

---

## Task 3: Wire the modal to styled, highlighted lines

**Files:** Modify `src/ui/modal.rs`, `src/app/input.rs`, `src/app/render.rs`, `src/ui/chronology_bar.rs`.

- [ ] **Step 1: Modal variant type**

In `src/ui/modal.rs`, change `Modal::ChangeDetail`'s field `lines: Vec<String>` → `lines: Vec<ratatui::text::Line<'static>>`. (The early-return guard and `unreachable!` arm match `ChangeDetail { .. }`, so they're unaffected.)

- [ ] **Step 2: `open_change_modal` builds styled lines**

In `src/app/input.rs` `open_change_modal`, replace:
```rust
    let lines = crate::ui::chronology_bar::change_detail_lines(&detail, line);
```
with:
```rust
    let lang = crate::ui::syntax::lang_for_path(&ev.file_path);
    let lines = crate::ui::syntax::change_detail_lines_styled(&detail, line, lang);
```
Everything else in the function (title, scroll, worktree/file/line) is unchanged. The modal input handler that reconstructs `Modal::ChangeDetail { …, lines, … }` and reads `lines.len()` works unchanged with `Vec<Line>`.

- [ ] **Step 3: Render styled lines**

In `src/app/render.rs` `render_change_detail_modal`:
- Change the signature param `lines: &[String]` → `lines: &[ratatui::text::Line<'static>]`.
- Replace the `visible` builder:
```rust
    let visible: Vec<Line> = lines
        .iter()
        .skip(scroll)
        .take(body_h)
        .map(|l| crate::ui::syntax::clip_line_to_width(l, inner.width as usize))
        .collect();
```
The dispatch call site `render_change_detail_modal(f, area, title, lines, *scroll, &app.theme)` is unchanged (now passes `&Vec<Line>`). Footer/clamp/scroll unchanged.

- [ ] **Step 4: Remove the superseded `change_detail_lines`**

In `src/ui/chronology_bar.rs`, delete `pub fn change_detail_lines` and its three tests (`change_detail_lines_edit_full_no_cap`, `change_detail_lines_write_numbers_all`, `change_detail_lines_none_is_empty`). Remove any now-unused imports that drops (e.g. if nothing else uses an import).

- [ ] **Step 5: Verify**

Run: `cargo build` (ZERO warnings — fix any unused import left by the removal), `cargo test --lib` (all pass), `cargo clippy --lib` (no new lints), `cargo fmt` + `cargo fmt --check`.
Manual: attach, open a Rust change in the modal → keywords magenta, strings yellow, comments gray, numbers cyan; `+` markers green, `-` red, gutter dim; scroll; narrow the terminal → lines clip without losing the gutter; open a `.txt`/no-ext change → plain (no colors), no crash.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(chronology): syntax-highlight + diff-tint the detail modal"
```

---

## Task 4: README

**Files:** Modify `README.md`.

- [ ] **Step 1: Document it**

In the "Change chronology" section's detail-modal description, add that the modal shows **basic syntax highlighting** (keywords/strings/comments/numbers, by file type — Rust, C-like, Python, Shell; other types shown plain) with **green `+` / red `-` diff markers**. Match the existing prose style; keep it to a sentence or two.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: note syntax highlighting in the chronology detail modal"
```

---

## Self-Review (completed during planning)

**Spec coverage:** `LangSpec`/`lang_for_path`/`highlight_code` (T1) ✓; palette (T1) ✓; `change_detail_lines_styled` styled builder w/ green/red markers + dim gutter + highlighted/plain code (T2) ✓; `clip_line_to_width` (T2) ✓; modal `lines: Vec<Line>` + open builds styled + render clips styled (T3) ✓; remove old `change_detail_lines` (T3) ✓; README (T4) ✓; syntax-first/colored-marker model (markers colored, code highlighted — T2) ✓; unknown ext → plain (T1 `lang_for_path` None + T2 `code_spans` None) ✓.

**Placeholder scan:** complete code in every code step; the only prose-only step (T4 README) is a doc edit. The `let _ = cm;` in `highlight_code` silences the unused `find` binding without changing behavior (line-comment prefix matched; rest emitted).

**Type consistency:** `LangSpec`, `lang_for_path(&Path) -> Option<&'static LangSpec>`, `highlight_code(&str, &LangSpec) -> Vec<Span<'static>>`, `change_detail_lines_styled(&ChangeDetail, u32, Option<&LangSpec>) -> Vec<Line<'static>>`, `clip_line_to_width(&Line<'static>, usize) -> Line<'static>`, `Modal::ChangeDetail.lines: Vec<Line<'static>>`, `render_change_detail_modal(.., lines: &[Line<'static>], ..)` — consistent across tasks.
