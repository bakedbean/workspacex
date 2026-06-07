# Chronology Modal Syntax Highlighting — Design

**Date:** 2026-06-06
**Status:** Approved for planning
**Builds on:** the chronology detail modal (`Modal::ChangeDetail`, `change_detail_lines`).

## Problem

The detail modal renders the change as plain, uncolored text. Code is harder to
scan without syntax coloring, and the added/removed sides aren't visually
distinct beyond the `+`/`-` glyph.

## Goal

Add **basic, dependency-free** syntax highlighting to the code shown in the
modal, plus a diff tint, without pulling in a heavy highlighter.

- Code tokens (keywords, strings, line comments, numbers) are colored, by
  language detected from the file extension.
- Diff tint is carried by the marker: green `+` / red `-`; the line-number
  gutter stays dim. (Syntax-first: every line's code is highlighted; whole-line
  green/red tinting is intentionally NOT used, because the peek is all `+`/`-`
  lines and full-line tint would hide the syntax colors.)
- No new crate dependencies (wsx keeps a lean dependency set).

## Decisions (from brainstorming)

- **A — lightweight hand-rolled highlighter** (not `syntect`). Zero new deps.
- **Syntax-first + colored marker** (not whole-line diff tint).
- Languages: **Rust**, generic **C-like**, **Python**, **Shell**; unknown →
  plain (no highlight).
- Per-line highlighting (no cross-line block-comment/multi-line-string state) —
  acceptable "basic" fidelity for a glanceable peek.

## Architecture

A new pure module `src/ui/syntax.rs` owns highlighting and the styled
change-line builder. The modal stores pre-highlighted styled `Line`s (computed
once on open); the renderer slices vertically and clips horizontally.

```
open modal: detail + file path
  → lang_for_path(file)                 -> Option<LangSpec>
  → change_detail_lines_styled(detail, base_line, lang)  -> Vec<Line<'static>>
       per line: dim gutter span · green/red marker span · highlight_code(code, lang) spans
  → Modal::ChangeDetail { lines: Vec<Line<'static>>, … }
render: clip_line_to_width(line, inner_width) per visible line
```

## Components (`src/ui/syntax.rs`, new — pure)

### Language spec + detection
```rust
/// A minimal language description driving the generic tokenizer.
pub struct LangSpec {
    pub keywords: &'static [&'static str],
    pub line_comment: &'static [&'static str], // e.g. ["//"], ["#"]
    pub string_delims: &'static [char],         // e.g. ['"', '\'']
}

/// Pick a `LangSpec` from a path's extension; `None` → no highlighting.
pub fn lang_for_path(path: &Path) -> Option<&'static LangSpec>;
```
Extension mapping:
- `rs` → RUST
- `c|h|cc|cpp|cxx|hpp|hh|js|jsx|ts|tsx|go|java|cs|json` → CLIKE
- `py` → PYTHON
- `sh|bash|zsh` → SHELL
- otherwise → `None`.

Static `LangSpec`s (representative keyword sets; not exhaustive):
- **RUST** keywords: `fn let mut pub use struct enum impl trait for in if else match while loop return self Self mod const static move ref as where async await dyn crate super type unsafe break continue true false`; line_comment `["//"]`; strings `['"']`.
- **CLIKE** keywords: `if else for while switch case break continue return struct class const static void int char bool new delete public private protected function var let import export from default true false null`; line_comment `["//"]`; strings `['"', '\'']`.
- **PYTHON** keywords: `def class return if elif else for while import from as with try except finally raise lambda None True False and or not in is pass yield global nonlocal`; line_comment `["#"]`; strings `['"', '\'']`.
- **SHELL** keywords: `if then else elif fi for in do done while case esac function return export local`; line_comment `["#"]`; strings `['"', '\'']`.

### Token highlighter
```rust
/// Tokenize ONE line of code into styled spans by `spec`. Recognizes (in
/// priority order): line comments (rest of line), strings (delim..delim with
/// `\` escape), numbers (leading-digit runs), keywords (whole identifiers),
/// else default. Pure; no cross-line state.
pub fn highlight_code(text: &str, spec: &LangSpec) -> Vec<Span<'static>>;
```
Single left-to-right scan over chars:
- At a `line_comment` prefix → emit the rest of the line as a comment span; stop.
- At a `string_delim` → consume to the matching delim (honoring `\` escapes) →
  string span.
- At an ASCII digit starting a token → consume the numeric run → number span.
- At an identifier start (`alpha`/`_`) → consume the identifier; if it's in
  `keywords` → keyword span, else default span.
- Else accumulate into a default span.
Adjacent default chars coalesce into one span.

### Palette
`ratatui::style::Color`: keyword = `Magenta`, string = `Yellow`,
comment = `DarkGray`, number = `Cyan`, default = unset fg. Marker `+` = `Green`,
`-` = `Red`. Gutter = `Modifier::DIM`. (Fixed palette; no theme plumbing.)

### Styled change-line builder
```rust
/// Build the modal's styled diff lines: each line is a dim 4-col gutter, a
/// green `+` / red `-` marker, then `highlight_code(code, lang)` (or a plain
/// span when `lang` is None). Added (`+`) lines numbered from `base_line`,
/// removed (`-`) lines blank gutter. No line cap.
pub fn change_detail_lines_styled(
    detail: &ChangeDetail,
    base_line: u32,
    lang: Option<&LangSpec>,
) -> Vec<Line<'static>>;
```
Replaces `chronology_bar::change_detail_lines` (only the modal used it; remove it
+ its tests). The gutter/marker format is unchanged (`"{n:>4} "` / 5-space
blank, then the marker), just split into styled spans instead of one string.

### Horizontal clip
```rust
/// Truncate a styled `Line` to `width` display columns, preserving span styles
/// (drops/trims spans past the limit). Char-based width.
pub fn clip_line_to_width(line: &Line<'static>, width: usize) -> Line<'static>;
```

## Integration

- `src/ui/mod.rs`: `pub mod syntax;`.
- `src/ui/modal.rs`: `Modal::ChangeDetail.lines: Vec<String>` → `Vec<ratatui::text::Line<'static>>`.
- `src/app/input.rs` `open_change_modal`: compute
  `let lang = crate::ui::syntax::lang_for_path(&ev.file_path);` and
  `let lines = crate::ui::syntax::change_detail_lines_styled(&detail, line, lang);`
  (replacing the `change_detail_lines` call). Everything else (title, scroll,
  worktree/file/line, `e`) unchanged.
- `src/app/render.rs` `render_change_detail_modal`: `lines: &[Line<'static>]`;
  each visible line rendered as `clip_line_to_width(line, inner.width as usize)`
  instead of `Span::raw(string.take(width))`. Footer/scroll/clamp unchanged.
- `src/ui/chronology_bar.rs`: remove `change_detail_lines` and its tests.

Note: the modal's stored `lines` are now styled `Line`s; the existing
clone-on-keystroke in the modal input handler clones them — acceptable at human
keystroke rate for one bounded change (documented; not optimized here).

## Error handling / edge cases

- Unknown extension / no extension → `lang_for_path` returns `None` →
  `change_detail_lines_styled` emits plain (default-styled) code spans. No error.
- Empty detail (`ChangeDetail::None`) → no lines (as today).
- Per-line scan only: a string/comment spanning multiple lines highlights each
  line independently (a trailing `"` or `/*` won't carry to the next line) —
  accepted "basic" limitation.
- `clip_line_to_width(_, 0)` → empty line; width ≥ content → unchanged.
- Non-ASCII: scanning is char-based; identifier/number checks use ASCII
  predicates, so non-ASCII chars fall into default spans (safe, no panic).

## Testing (pure)

- **`lang_for_path`**: `a.rs`→RUST, `a.py`→PYTHON, `a.c`/`a.js`→CLIKE,
  `a.sh`→SHELL, `a.txt`/no-ext→None.
- **`highlight_code`** (RUST): `let x = "hi"; // c` → a `let` keyword span, a
  `"hi"` string span, a `// c` comment span (rest of line), and `42` →
  number span; assert the styled segments carry the right `Color`.
- **string escape**: `"a\"b"` stays one string span.
- **`change_detail_lines_styled`**: `+` line marker is Green and numbered from
  `base_line`; `-` line marker is Red with blank gutter; gutter span is DIM;
  with `lang=None` the code is a single default span; with RUST, a keyword in
  the code is Magenta.
- **`clip_line_to_width`**: clipping mid-span keeps the earlier spans' styles and
  trims the boundary span; width 0 → empty; wide width → identical spans.
- Modal render/input glue: build + manual (open a Rust change → colored tokens,
  green/red markers; scroll; narrow terminal clips without losing the gutter).

## Files touched

- `src/ui/syntax.rs` (new) — `LangSpec`, `lang_for_path`, `highlight_code`,
  `change_detail_lines_styled`, `clip_line_to_width`, palette + tests.
- `src/ui/mod.rs` — `pub mod syntax;`.
- `src/ui/modal.rs` — `ChangeDetail.lines` type → `Vec<Line<'static>>`.
- `src/app/input.rs` — `open_change_modal` uses the styled builder + lang.
- `src/app/render.rs` — render styled lines via `clip_line_to_width`.
- `src/ui/chronology_bar.rs` — remove `change_detail_lines` (+ tests).
- `README.md` — note basic syntax highlighting + diff-tinted markers in the modal.
