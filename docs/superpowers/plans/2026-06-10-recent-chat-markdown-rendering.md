# RECENT CHAT Markdown Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the RECENT CHAT detail-bar module's assistant text as formatted markdown — paragraph spacing, headings, lists, code blocks, blockquotes, and inline bold/italic/code — using spacing, weight, and accent colors for legibility.

**Architecture:** A new pure module `src/detail_modules/markdown.rs` exposes `render(text, width, theme) -> Vec<Line<'static>>`. It parses with `pulldown-cmark` into a private `Vec<Block>` intermediate representation, then a token-aware greedy wrapper turns each block into width-wrapped, per-span-styled `Line`s, joined with blank-line gaps. `recent_chat.rs` shrinks to delegate to it; its old string-based `wrap_lines` is deleted. Styling lives in `theme.rs` behind new `md_*` helpers plus one new `code: Color` field.

**Tech Stack:** Rust (edition 2024), ratatui 0.29, pulldown-cmark 0.13.

**Spec:** `docs/superpowers/specs/2026-06-10-recent-chat-markdown-rendering-design.md`

---

## File structure

- **Create** `src/detail_modules/markdown.rs` — the renderer (parse → wrap → style). Self-contained, no `DetailContext` dependency.
- **Modify** `src/detail_modules/mod.rs` — add `pub mod markdown;`.
- **Modify** `src/detail_modules/recent_chat.rs` — delegate to `markdown::render`; delete `wrap_lines` and its tests.
- **Modify** `src/ui/theme.rs` — add `code: Color` field to `Theme` + all five constructors; add `md_heading_style`, `md_code_style`, `md_bullet_style`, `md_quote_style`.
- **Modify** `Cargo.toml` — add `pulldown-cmark`.

### Shared type definitions (defined in Task 2/3, referenced throughout)

```rust
// src/detail_modules/markdown.rs

#[derive(Clone)]
enum Tok {
    Run { text: String, style: ratatui::style::Style },
    Break,
}

#[derive(Clone, Copy)]
enum Marker {
    Bullet,
    Number(u64),
}

enum Block {
    Paragraph(Vec<Tok>),
    Heading(Vec<Tok>),
    ListItem { marker: Marker, body: Vec<Tok> },
    Code(Vec<String>),
    Quote(Vec<Tok>),
}
```

> **pulldown-cmark version note:** This plan targets 0.13, where `Tag::BlockQuote(Option<BlockQuoteKind>)`, `Tag::List(Option<u64>)`, `Tag::CodeBlock(CodeBlockKind)`, and `Tag::Heading { .. }` hold these shapes. The walker matches `Event::End(_)` with a wildcard and tracks open/close via its own frame stack, so it never destructures `TagEnd` payloads — only the four `Tag::` *start* patterns above are version-sensitive. If a patch bump changes one, adjust that single `match` arm.

---

## Task 1: Add dependency and theme styling support

**Files:**
- Modify: `Cargo.toml` (the `[dependencies]` block)
- Modify: `src/ui/theme.rs` (struct `Theme`, five constructors, style helpers)
- Test: `src/ui/theme.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, under `[dependencies]` (after the `shlex = "2"` line), add:

```toml
pulldown-cmark = { version = "0.13", default-features = false }
```

`default-features = false` drops the `html` rendering feature and its `getopts` CLI dep; the event-stream parser we use is always available.

- [ ] **Step 2: Run a build to fetch the crate**

Run: `cargo build`
Expected: PASS (compiles; downloads pulldown-cmark on first run).

- [ ] **Step 3: Write the failing theme test**

In `src/ui/theme.rs`, inside the existing `#[cfg(test)] mod tests` block, add:

```rust
#[test]
fn markdown_styles_are_distinct_and_weighted() {
    use ratatui::style::Modifier;
    let t = Theme::wsx();

    // Heading is brighter than body and bold.
    let h = t.md_heading_style();
    assert_eq!(h.fg, Some(t.header_fg));
    assert!(h.add_modifier.contains(Modifier::BOLD));

    // Code has its own color, distinct from the body grey.
    assert_ne!(t.code, t.dim);
    assert_eq!(t.md_code_style().fg, Some(t.code));

    // Bullet marker uses the accent color.
    assert_eq!(t.md_bullet_style().fg, Some(t.attention));

    // Blockquote body is dim + italic.
    let q = t.md_quote_style();
    assert_eq!(q.fg, Some(t.dim));
    assert!(q.add_modifier.contains(Modifier::ITALIC));
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test --lib theme::tests::markdown_styles_are_distinct_and_weighted`
Expected: FAIL — compile error, `no field code on Theme` / `no method md_heading_style`.

- [ ] **Step 5: Add the `code` field to the struct**

In `src/ui/theme.rs`, in the `pub struct Theme { .. }` definition, add after the `pub path: Color,` line:

```rust
    /// Inline-code and code-block foreground for rendered markdown.
    /// A distinct, legible hue — never darker than `dim`.
    pub code: Color,
```

- [ ] **Step 6: Set `code` in all five constructors**

Add a `code:` line to each `Self { .. }` literal (place it next to the existing `path:` line):

- `ansi()`: `code: Color::Cyan,`
- `wsx()`: `code: Color::Rgb(0x7e, 0xb6, 0xb0), // teal`
- `dracula()`: `code: cyan,` (the `cyan` local already defined at the top of `dracula()`)
- `jellybeans()`: `code: green,` (the `green` local already defined in `jellybeans()`)
- `nord()`: `code: Color::Rgb(0x8f, 0xbc, 0xbb), // nord frost2`

- [ ] **Step 7: Add the style helper methods**

In `src/ui/theme.rs`, in the `impl Theme` block next to `dim_style`/`path_style`, add:

```rust
    /// Heading line in rendered markdown — brighter than body, bold.
    pub fn md_heading_style(&self) -> Style {
        Style::default()
            .fg(self.header_fg)
            .add_modifier(Modifier::BOLD)
    }
    /// Inline code and fenced code blocks in rendered markdown.
    pub fn md_code_style(&self) -> Style {
        Style::default().fg(self.code)
    }
    /// List-item bullet/number marker in rendered markdown.
    pub fn md_bullet_style(&self) -> Style {
        Style::default().fg(self.attention)
    }
    /// Blockquote body in rendered markdown — dim and italic.
    pub fn md_quote_style(&self) -> Style {
        Style::default()
            .fg(self.dim)
            .add_modifier(Modifier::ITALIC)
    }
```

(`Style` and `Modifier` are already imported at the top of `theme.rs`.)

- [ ] **Step 8: Run the test to verify it passes**

Run: `cargo test --lib theme::tests::markdown_styles_are_distinct_and_weighted`
Expected: PASS.

- [ ] **Step 9: Run the full theme test module and fmt**

Run: `cargo fmt && cargo test --lib theme`
Expected: PASS (all existing theme tests still green).

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src/ui/theme.rs
git commit -m "feat(theme): add markdown style helpers and code color"
```

---

## Task 2: Token-aware word wrapper

**Files:**
- Create: `src/detail_modules/markdown.rs`
- Modify: `src/detail_modules/mod.rs` (register the module)
- Test: `src/detail_modules/markdown.rs` (`#[cfg(test)] mod tests`)

This task builds the pure wrapping core with no pulldown dependency yet, so it is testable in isolation.

- [ ] **Step 1: Create the module file with types and wrapper, plus failing tests**

Create `src/detail_modules/markdown.rs` with:

```rust
//! Markdown renderer for assistant text in the RECENT CHAT detail
//! module. Parses with `pulldown-cmark`, then wraps to the column width
//! ourselves, applying per-span styling for headings, lists, code,
//! blockquotes, and inline emphasis. Pure: `&str + width + &Theme` in,
//! `Vec<Line<'static>>` out.

use ratatui::style::Style;
use ratatui::text::Span;

#[derive(Clone)]
enum Tok {
    Run { text: String, style: Style },
    Break,
}

/// Greedily pack inline tokens into logical lines of at most `avail`
/// columns, counted in `char`s to match the rest of the TUI's wrapping.
/// `Tok::Break` forces a new line; words longer than `avail` hard-split.
fn wrap_words(tokens: &[Tok], avail: usize) -> Vec<Vec<(String, Style)>> {
    let avail = avail.max(1);
    let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
    let mut cur: Vec<(String, Style)> = Vec::new();
    let mut cur_len = 0usize;
    for tok in tokens {
        match tok {
            Tok::Break => {
                lines.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            Tok::Run { text, style } => {
                for word in text.split_whitespace() {
                    let mut remaining: &str = word;
                    // Hard-split a word longer than the whole column.
                    while remaining.chars().count() > avail {
                        if !cur.is_empty() {
                            lines.push(std::mem::take(&mut cur));
                            cur_len = 0;
                        }
                        let head: String = remaining.chars().take(avail).collect();
                        lines.push(vec![(head, *style)]);
                        remaining = char_slice_from(remaining, avail);
                    }
                    if remaining.is_empty() {
                        continue;
                    }
                    let wlen = remaining.chars().count();
                    let projected = if cur.is_empty() { wlen } else { cur_len + 1 + wlen };
                    if projected > avail && !cur.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        cur_len = 0;
                    }
                    cur_len = if cur.is_empty() { wlen } else { cur_len + 1 + wlen };
                    cur.push((remaining.to_string(), *style));
                }
            }
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Byte-safe `&str` slice starting at the `n`th `char`.
fn char_slice_from(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((idx, _)) => &s[idx..],
        None => "",
    }
}

/// Merge a logical line's words into ratatui spans, coalescing adjacent
/// words that share a `Style` into one span and joining with spaces.
fn coalesce(words: &[(String, Style)]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut buf_style: Option<Style> = None;
    for (i, (word, style)) in words.iter().enumerate() {
        let sep = if i == 0 { "" } else { " " };
        match buf_style {
            Some(s) if s == *style => {
                buf.push_str(sep);
                buf.push_str(word);
            }
            _ => {
                if let Some(s) = buf_style {
                    spans.push(Span::styled(std::mem::take(&mut buf), s));
                }
                buf.push_str(sep);
                buf.push_str(word);
                buf_style = Some(*style);
            }
        }
    }
    if let Some(s) = buf_style {
        spans.push(Span::styled(buf, s));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Tok {
        Tok::Run { text: text.to_string(), style: Style::default() }
    }

    #[test]
    fn wraps_on_word_boundary() {
        // avail 11: "hello world" (11) fits; "foo" overflows to line 2.
        let lines = wrap_words(&[run("hello world foo")], 11);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 2); // hello, world
        assert_eq!(lines[1].len(), 1); // foo
    }

    #[test]
    fn hard_splits_overlong_word() {
        let lines = wrap_words(&[run("abcdefgh")], 3);
        let joined: Vec<String> = lines.iter().map(|l| l[0].0.clone()).collect();
        assert_eq!(joined, vec!["abc", "def", "gh"]);
    }

    #[test]
    fn break_forces_new_line() {
        let lines = wrap_words(&[run("a"), Tok::Break, run("b")], 80);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].0, "a");
        assert_eq!(lines[1][0].0, "b");
    }

    #[test]
    fn coalesce_merges_same_style() {
        let dim = Style::default();
        let words = vec![("a".to_string(), dim), ("b".to_string(), dim)];
        let spans = coalesce(&words);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "a b");
    }

    #[test]
    fn coalesce_splits_on_style_change() {
        use ratatui::style::Color;
        let a = Style::default().fg(Color::Red);
        let b = Style::default().fg(Color::Blue);
        let words = vec![("a".to_string(), a), ("b".to_string(), b)];
        let spans = coalesce(&words);
        assert_eq!(spans.len(), 2);
    }
}
```

- [ ] **Step 2: Register the module**

In `src/detail_modules/mod.rs`, add to the `pub mod` list (near `pub mod recent_chat;`):

```rust
pub mod markdown;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --lib detail_modules::markdown`
Expected: PASS (5 tests).

- [ ] **Step 4: Commit**

```bash
git add src/detail_modules/markdown.rs src/detail_modules/mod.rs
git commit -m "feat(markdown): token-aware word wrapper"
```

---

## Task 3: Parse markdown into the block IR

**Files:**
- Modify: `src/detail_modules/markdown.rs` (add types + `parse_blocks` and helpers)
- Test: `src/detail_modules/markdown.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Add the block/marker types and parser, with failing tests**

In `src/detail_modules/markdown.rs`, update the imports at the top to:

```rust
use crate::ui::theme::Theme;
use pulldown_cmark::{Event, Options, Parser, Tag};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
```

Add the `Marker` and `Block` types just below the existing `Tok` enum:

```rust
#[derive(Clone, Copy)]
enum Marker {
    Bullet,
    Number(u64),
}

enum Block {
    Paragraph(Vec<Tok>),
    Heading(Vec<Tok>),
    ListItem { marker: Marker, body: Vec<Tok> },
    Code(Vec<String>),
    Quote(Vec<Tok>),
}
```

Add these functions (above the `#[cfg(test)]` block):

```rust
/// Base foreground style for inline text given the current block context.
fn base_style(heading: bool, quote: bool, theme: &Theme) -> Style {
    if heading {
        theme.md_heading_style()
    } else if quote {
        theme.md_quote_style()
    } else {
        theme.dim_style()
    }
}

/// Build a block from accumulated inline tokens, choosing the variant
/// from context. Returns `None` for content-free runs (e.g. the empty
/// flush at the end of a loose list item) so we don't emit blank blocks.
fn make_block(toks: Vec<Tok>, heading: bool, quote: bool, marker: Option<Marker>) -> Option<Block> {
    let has_content = toks.iter().any(|t| match t {
        Tok::Run { text, .. } => !text.trim().is_empty(),
        Tok::Break => false,
    });
    if !has_content {
        return None;
    }
    Some(if heading {
        Block::Heading(toks)
    } else if let Some(m) = marker {
        Block::ListItem { marker: m, body: toks }
    } else if quote {
        Block::Quote(toks)
    } else {
        Block::Paragraph(toks)
    })
}

/// Walk the pulldown event stream into a flat list of styled blocks.
/// Block open/close is tracked with our own frame stack so `End` events
/// never need their payload destructured (version-robust).
fn parse_blocks(text: &str, theme: &Theme) -> Vec<Block> {
    enum Frame {
        Emph(Modifier),
        Heading,
        Item,
        Code,
        Quote,
        List,
        Para,
        Other,
    }

    let parser = Parser::new_ext(text, Options::empty());

    let mut blocks: Vec<Block> = Vec::new();
    let mut toks: Vec<Tok> = Vec::new();
    let mut code_buf = String::new();
    let mut mods = Modifier::empty();
    let mut heading = false;
    let mut quote = false;
    let mut code = false;
    let mut cur_marker: Option<Marker> = None;
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => {
                let frame = match tag {
                    Tag::Strong => {
                        mods |= Modifier::BOLD;
                        Frame::Emph(Modifier::BOLD)
                    }
                    Tag::Emphasis => {
                        mods |= Modifier::ITALIC;
                        Frame::Emph(Modifier::ITALIC)
                    }
                    Tag::Heading { .. } => {
                        heading = true;
                        Frame::Heading
                    }
                    Tag::BlockQuote(_) => {
                        quote = true;
                        Frame::Quote
                    }
                    Tag::CodeBlock(_) => {
                        code = true;
                        code_buf.clear();
                        Frame::Code
                    }
                    Tag::List(first) => {
                        list_stack.push(first);
                        Frame::List
                    }
                    Tag::Item => {
                        let marker = match list_stack.last_mut() {
                            Some(Some(n)) => {
                                let v = *n;
                                *n += 1;
                                Marker::Number(v)
                            }
                            _ => Marker::Bullet,
                        };
                        cur_marker = Some(marker);
                        Frame::Item
                    }
                    Tag::Paragraph => Frame::Para,
                    _ => Frame::Other,
                };
                frames.push(frame);
            }
            Event::End(_) => match frames.pop() {
                Some(Frame::Emph(m)) => mods.remove(m),
                Some(Frame::Heading) => {
                    if let Some(b) = make_block(std::mem::take(&mut toks), heading, quote, cur_marker) {
                        blocks.push(b);
                    }
                    heading = false;
                }
                Some(Frame::Para) => {
                    if let Some(b) = make_block(std::mem::take(&mut toks), heading, quote, cur_marker) {
                        blocks.push(b);
                    }
                }
                Some(Frame::Item) => {
                    if let Some(b) = make_block(std::mem::take(&mut toks), heading, quote, cur_marker) {
                        blocks.push(b);
                    }
                    cur_marker = None;
                }
                Some(Frame::Quote) => {
                    if let Some(b) = make_block(std::mem::take(&mut toks), heading, quote, cur_marker) {
                        blocks.push(b);
                    }
                    quote = false;
                }
                Some(Frame::Code) => {
                    let body = code_buf.strip_suffix('\n').unwrap_or(&code_buf);
                    let lines: Vec<String> = body.split('\n').map(|s| s.to_string()).collect();
                    if !(lines.len() == 1 && lines[0].is_empty()) {
                        blocks.push(Block::Code(lines));
                    }
                    code = false;
                }
                Some(Frame::List) => {
                    list_stack.pop();
                }
                _ => {}
            },
            Event::Text(s) => {
                if code {
                    code_buf.push_str(&s);
                } else {
                    let style = base_style(heading, quote, theme).add_modifier(mods);
                    toks.push(Tok::Run { text: s.into_string(), style });
                }
            }
            Event::Code(s) => {
                toks.push(Tok::Run { text: s.into_string(), style: theme.md_code_style() });
            }
            Event::SoftBreak => {
                toks.push(Tok::Run { text: " ".to_string(), style: theme.dim_style() });
            }
            Event::HardBreak => toks.push(Tok::Break),
            _ => {}
        }
    }
    blocks
}
```

Add these tests inside the `#[cfg(test)] mod tests` block:

```rust
    use crate::ui::theme::Theme;

    fn run_text(toks: &[Tok]) -> String {
        toks.iter()
            .filter_map(|t| match t {
                Tok::Run { text, .. } => Some(text.clone()),
                Tok::Break => None,
            })
            .collect()
    }

    #[test]
    fn parses_bold_into_paragraph_with_bold_span() {
        let t = Theme::wsx();
        let blocks = parse_blocks("hello **world**", &t);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Paragraph(toks) => {
                let bold = toks.iter().any(|tok| matches!(
                    tok,
                    Tok::Run { text, style }
                        if text.trim() == "world" && style.add_modifier.contains(Modifier::BOLD)
                ));
                assert!(bold, "expected a bold 'world' run");
            }
            _ => panic!("expected a paragraph"),
        }
    }

    #[test]
    fn parses_heading() {
        let t = Theme::wsx();
        let blocks = parse_blocks("## Next steps", &t);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Heading(_)));
    }

    #[test]
    fn parses_bullet_and_numbered_lists() {
        let t = Theme::wsx();
        let blocks = parse_blocks("- one\n- two", &t);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::ListItem { marker: Marker::Bullet, .. }));

        let ordered = parse_blocks("1. first\n2. second", &t);
        assert!(matches!(&ordered[0], Block::ListItem { marker: Marker::Number(1), .. }));
        assert!(matches!(&ordered[1], Block::ListItem { marker: Marker::Number(2), .. }));
    }

    #[test]
    fn parses_code_block_verbatim() {
        let t = Theme::wsx();
        let blocks = parse_blocks("```\nlet x = **1**;\n```", &t);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            // Markdown inside a fence stays literal — not parsed as bold.
            Block::Code(lines) => assert_eq!(lines, &vec!["let x = **1**;".to_string()]),
            _ => panic!("expected a code block"),
        }
    }

    #[test]
    fn parses_blockquote() {
        let t = Theme::wsx();
        let blocks = parse_blocks("> quoted", &t);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Quote(toks) => assert_eq!(run_text(toks).trim(), "quoted"),
            _ => panic!("expected a quote"),
        }
    }
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test --lib detail_modules::markdown`
Expected: PASS (Task 2 tests + 5 new parse tests).

- [ ] **Step 3: Commit**

```bash
git add src/detail_modules/markdown.rs
git commit -m "feat(markdown): parse event stream into styled block IR"
```

---

## Task 4: Assemble `render` and wire into RECENT CHAT

**Files:**
- Modify: `src/detail_modules/markdown.rs` (add `flow_lines`, `block_to_lines`, `render`, integration tests)
- Modify: `src/detail_modules/recent_chat.rs` (delegate; delete `wrap_lines` + its tests)
- Test: `src/detail_modules/markdown.rs` and `src/detail_modules/recent_chat.rs`

- [ ] **Step 1: Add the line-assembly + public render fns, with failing tests**

In `src/detail_modules/markdown.rs`, add `Line` to the ratatui import:

```rust
use ratatui::text::{Line, Span};
```

Add these functions (above the `#[cfg(test)]` block):

```rust
/// Wrap inline tokens to `width`, prefixing the first line with `lead`
/// and continuation lines with `cont` (used for list hanging indents and
/// blockquote bars). `lead` and `cont` are assumed to be the same width.
fn flow_lines(tokens: &[Tok], width: usize, lead: Span<'static>, cont: Span<'static>) -> Vec<Line<'static>> {
    let indent = lead.content.chars().count();
    let avail = width.saturating_sub(indent);
    let logical = wrap_words(tokens, avail);
    let mut out = Vec::new();
    for (i, words) in logical.iter().enumerate() {
        let mut spans = vec![if i == 0 { lead.clone() } else { cont.clone() }];
        spans.extend(coalesce(words));
        out.push(Line::from(spans));
    }
    out
}

/// Render one block to its display lines (no inter-block spacing).
fn block_to_lines(block: &Block, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    match block {
        Block::Paragraph(toks) | Block::Heading(toks) => {
            flow_lines(toks, width, Span::raw(""), Span::raw(""))
        }
        Block::Quote(toks) => {
            let bar = Span::styled("│ ".to_string(), theme.path_style());
            flow_lines(toks, width, bar.clone(), bar)
        }
        Block::ListItem { marker, body } => {
            let lead_text = match marker {
                Marker::Bullet => "• ".to_string(),
                Marker::Number(n) => format!("{n}. "),
            };
            let cont = Span::raw(" ".repeat(lead_text.chars().count()));
            let lead = Span::styled(lead_text, theme.md_bullet_style());
            flow_lines(body, width, lead, cont)
        }
        Block::Code(lines) => lines
            .iter()
            .map(|l| {
                let truncated: String = l.chars().take(width.saturating_sub(2)).collect();
                Line::from(vec![Span::raw("  "), Span::styled(truncated, theme.md_code_style())])
            })
            .collect(),
    }
}

/// Render markdown `text` into styled, width-wrapped lines for the
/// detail bar. Blocks are separated by a single blank line; there are no
/// leading or trailing blanks (the host owns inter-module spacing).
pub fn render(text: &str, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from(Span::styled(text.to_string(), theme.dim_style()))];
    }
    let width = width as usize;
    let mut out: Vec<Line<'static>> = Vec::new();
    for block in &parse_blocks(text, theme) {
        let lines = block_to_lines(block, width, theme);
        if lines.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(Line::from(""));
        }
        out.extend(lines);
    }
    out
}
```

Add these integration tests inside the `#[cfg(test)] mod tests` block. Helper to flatten a `Line` to plain text:

```rust
    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn render_separates_blocks_with_blank_line_no_edge_blanks() {
        let t = Theme::wsx();
        let out = render("para one\n\n## Heading", 40, &t);
        // paragraph, blank, heading — no leading/trailing blank.
        assert!(!line_text(&out[0]).is_empty());
        assert!(out.iter().any(|l| line_text(l).is_empty()));
        assert!(!line_text(out.last().unwrap()).is_empty());
    }

    #[test]
    fn render_bullet_has_marker_and_hanging_indent() {
        let t = Theme::wsx();
        // Narrow width forces the item to wrap onto a continuation line.
        let out = render("- alpha beta gamma delta epsilon", 14, &t);
        assert!(line_text(&out[0]).starts_with("• "));
        // Continuation line is indented by the 2-cell marker width.
        assert!(out.len() > 1);
        assert!(line_text(&out[1]).starts_with("  "));
        assert!(!line_text(&out[1]).starts_with("• "));
    }

    #[test]
    fn render_code_block_is_indented_and_colored() {
        let t = Theme::wsx();
        let out = render("```\nfn main() {}\n```", 40, &t);
        assert_eq!(out.len(), 1);
        assert!(line_text(&out[0]).starts_with("  fn main() {}"));
        // Body span carries the code color.
        let colored = out[0].spans.iter().any(|s| s.style.fg == Some(t.code));
        assert!(colored);
    }

    #[test]
    fn render_inline_code_uses_code_color() {
        let t = Theme::wsx();
        let out = render("call `validate_token` now", 40, &t);
        let has_code = out
            .iter()
            .flat_map(|l| l.spans.iter())
            .any(|s| s.content.contains("validate_token") && s.style.fg == Some(t.code));
        assert!(has_code);
    }

    #[test]
    fn render_plain_prose_wraps_like_before() {
        let t = Theme::wsx();
        let out = render("the quick brown fox jumps", 9, &t);
        // "the quick" (9) then wraps. Every line within width.
        assert!(out.iter().all(|l| line_text(l).chars().count() <= 9));
        assert!(out.len() >= 3);
    }

    #[test]
    fn render_width_zero_does_not_panic() {
        let t = Theme::wsx();
        let out = render("anything **here**", 0, &t);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn render_empty_input_is_empty() {
        let t = Theme::wsx();
        assert!(render("", 40, &t).is_empty());
        assert!(render("   \n  ", 40, &t).is_empty());
    }
```

- [ ] **Step 2: Run the markdown tests to verify they pass**

Run: `cargo test --lib detail_modules::markdown`
Expected: PASS (all prior + 7 new integration tests).

- [ ] **Step 3: Rewrite `recent_chat.rs` to delegate**

Replace the body of `src/detail_modules/recent_chat.rs` from the `fn build_lines` line through the end of `wrap_lines` (i.e. delete `wrap_lines` entirely and its use inside `build_lines`) with:

```rust
fn build_lines(ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::text::{Line, Span};

    let events = if ctx.events_scanned { ctx.events } else { None };
    let theme = ctx.theme;

    let Some(evt) = events else {
        return vec![Line::from(Span::styled("  loading…".to_string(), theme.dim_style()))];
    };

    let Some(text) = evt.last_assistant_text.as_deref() else {
        return vec![Line::from(Span::styled("—".to_string(), theme.dim_style()))];
    };

    crate::detail_modules::markdown::render(text, width, theme)
}
```

Then, in the `#[cfg(test)] mod tests` block of `recent_chat.rs`, **delete** any test that calls `wrap_lines` (the wrapping logic is now covered by `markdown`'s tests). Keep `id_is_recent_chat`, `title_is_uppercase`, and `lines_with_no_events_returns_at_least_one_line`.

- [ ] **Step 4: Run the recent_chat tests**

Run: `cargo test --lib detail_modules::recent_chat`
Expected: PASS (3 tests; no `wrap_lines` references remain).

- [ ] **Step 5: Run the full suite, clippy, and fmt**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS — no warnings, all tests green.

- [ ] **Step 6: Commit**

```bash
git add src/detail_modules/markdown.rs src/detail_modules/recent_chat.rs
git commit -m "feat(recent-chat): render assistant text as markdown"
```

---

## Task 5: Manual verification

**Files:** none (runtime check)

- [ ] **Step 1: Build and run the app**

Run: `cargo run`
Then select a workspace whose agent has produced a markdown-formatted assistant message (bold, a list, inline code, a heading).

- [ ] **Step 2: Confirm the RECENT CHAT module**

Visually verify:
- Bold/italic/inline-code render with weight/color, not literal `**`/`` ` `` markers.
- Headings appear brighter and bold, with a blank line above.
- Bullet/numbered lists show `•`/`1.` markers in the accent color with wrapped lines hanging-indented.
- Code blocks are indented and code-colored; blockquotes show the `│ ` bar.
- Blocks are separated by blank lines; no doubled gap above the next module.

- [ ] **Step 3: Confirm empty/loading states unchanged**

Select a workspace mid-scan (shows `  loading…`) and one with no assistant text (shows `—`). Both should render exactly as before.

---

## Self-review notes

- **Spec coverage:** inline emphasis (Task 3 `parses_bold…`, Task 4 `render_inline_code…`), headings (Task 3 `parses_heading`, Task 4 blank-line test), lists incl. hanging indent + numbering (Task 3 + Task 4 `render_bullet…`), code blocks verbatim (Task 3 + Task 4), blockquotes (Task 3 + `block_to_lines`), token-aware wrapping + hard-split (Task 2), accent colors + new `code` field (Task 1), deleted `wrap_lines` regression guard (Task 4 `render_plain_prose…`), `width==0` + empty input guards (Task 4). All spec sections map to a task.
- **Type consistency:** `Tok`, `Marker`, `Block` defined once (Tasks 2–3), used unchanged in Task 4. `render(text, width, theme)` signature matches the spec and the `recent_chat` call site. `md_heading_style`/`md_code_style`/`md_bullet_style`/`md_quote_style` and the `code` field defined in Task 1 are the exact names used in Tasks 3–4.
- **No placeholders:** every code step shows complete code; every run step states the expected result.
