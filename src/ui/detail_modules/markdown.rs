//! Markdown renderer for assistant text in the RECENT CHAT detail
//! module. Parses with `pulldown-cmark`, then wraps to the column width
//! ourselves, applying per-span styling for headings, lists, code,
//! blockquotes, and inline emphasis. Pure: `&str + width + &Theme` in,
//! `Vec<Line<'static>>` out.

use crate::ui::theme::Theme;
use pulldown_cmark::{Event, Options, Parser, Tag};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Clone, Debug)]
enum Tok {
    Run { text: String, style: Style },
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
                    let projected = if cur.is_empty() {
                        wlen
                    } else {
                        cur_len + 1 + wlen
                    };
                    if projected > avail && !cur.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        cur_len = 0;
                    }
                    cur_len = if cur.is_empty() {
                        wlen
                    } else {
                        cur_len + 1 + wlen
                    };
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
/// Inter-word spaces are always preserved; at a style boundary the
/// separating space is attached to the following span.
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
        Block::ListItem {
            marker: m,
            body: toks,
        }
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
                    if let Some(b) =
                        make_block(std::mem::take(&mut toks), heading, quote, cur_marker)
                    {
                        blocks.push(b);
                    }
                    heading = false;
                }
                Some(Frame::Para) => {
                    if let Some(b) =
                        make_block(std::mem::take(&mut toks), heading, quote, cur_marker)
                    {
                        blocks.push(b);
                    }
                    // Only the first paragraph of a loose list item carries the
                    // marker; later paragraphs of the same item fall back to plain
                    // paragraphs rather than repeating the bullet/number.
                    cur_marker = None;
                }
                Some(Frame::Item) => {
                    if let Some(b) =
                        make_block(std::mem::take(&mut toks), heading, quote, cur_marker)
                    {
                        blocks.push(b);
                    }
                    cur_marker = None;
                }
                Some(Frame::Quote) => {
                    if let Some(b) =
                        make_block(std::mem::take(&mut toks), heading, quote, cur_marker)
                    {
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
                    toks.push(Tok::Run {
                        text: s.into_string(),
                        style,
                    });
                }
            }
            Event::Code(s) => {
                toks.push(Tok::Run {
                    text: s.into_string(),
                    style: theme.md_code_style(),
                });
            }
            Event::SoftBreak => {
                toks.push(Tok::Run {
                    text: " ".to_string(),
                    style: theme.dim_style(),
                });
            }
            Event::HardBreak => toks.push(Tok::Break),
            _ => {}
        }
    }
    blocks
}

/// Wrap inline tokens to `width`, prefixing the first line with `lead`
/// and continuation lines with `cont` (used for list hanging indents and
/// blockquote bars). `lead` and `cont` are assumed to be the same width.
///
/// The `width` bound holds for any realistic detail-column width. When
/// `width` is smaller than the prefix itself (a degenerate case the detail
/// bar never produces — it is dozens of columns wide), a line may exceed
/// `width` by up to the prefix width; the terminal clips the overflow.
fn flow_lines(
    tokens: &[Tok],
    width: usize,
    lead: Span<'static>,
    cont: Span<'static>,
) -> Vec<Line<'static>> {
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
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(truncated, theme.md_code_style()),
                ])
            })
            .collect(),
    }
}

/// Render markdown `text` into styled, width-wrapped lines for the
/// detail bar. Blocks are separated by a single blank line; there are no
/// leading or trailing blanks (the host owns inter-module spacing).
///
/// Lines are wrapped to `width`. For prefixed blocks (lists, blockquotes,
/// code) at a width narrower than the prefix — a degenerate case the
/// detail column never reaches — a line may exceed `width` (see
/// `flow_lines`). The terminal clips any such overflow.
pub fn render(text: &str, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from(Span::styled(
            text.to_string(),
            theme.dim_style(),
        ))];
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Tok {
        Tok::Run {
            text: text.to_string(),
            style: Style::default(),
        }
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
        // The inter-word space must survive the style boundary.
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "a b");
    }

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
                let bold = toks.iter().any(|tok| {
                    matches!(
                        tok,
                        Tok::Run { text, style }
                            if text.trim() == "world" && style.add_modifier.contains(Modifier::BOLD)
                    )
                });
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
        assert!(matches!(
            &blocks[0],
            Block::ListItem {
                marker: Marker::Bullet,
                ..
            }
        ));

        let ordered = parse_blocks("1. first\n2. second", &t);
        assert!(matches!(
            &ordered[0],
            Block::ListItem {
                marker: Marker::Number(1),
                ..
            }
        ));
        assert!(matches!(
            &ordered[1],
            Block::ListItem {
                marker: Marker::Number(2),
                ..
            }
        ));
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
    fn loose_list_item_marks_only_first_paragraph() {
        let t = Theme::wsx();
        // A single bullet item split into two paragraphs (loose list).
        let blocks = parse_blocks("- one\n\n  two", &t);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(
            &blocks[0],
            Block::ListItem {
                marker: Marker::Bullet,
                ..
            }
        ));
        // The continuation paragraph must NOT repeat the marker.
        assert!(matches!(&blocks[1], Block::Paragraph(_)));
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

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn render_separates_blocks_with_blank_line_no_edge_blanks() {
        let t = Theme::wsx();
        let out = render("para one\n\n## Heading", 40, &t);
        assert!(!line_text(&out[0]).is_empty());
        assert!(out.iter().any(|l| line_text(l).is_empty()));
        assert!(!line_text(out.last().unwrap()).is_empty());
    }

    #[test]
    fn render_bullet_has_marker_and_hanging_indent() {
        let t = Theme::wsx();
        let out = render("- alpha beta gamma delta epsilon", 14, &t);
        assert!(line_text(&out[0]).starts_with("• "));
        assert!(out.len() > 1);
        assert!(line_text(&out[1]).starts_with("  "));
        assert!(!line_text(&out[1]).starts_with("• "));
    }

    #[test]
    fn render_bullet_lines_stay_within_width() {
        let t = Theme::wsx();
        // A long bullet item at a realistic narrow width: every rendered
        // line (marker + text, or hanging indent + text) must fit `width`,
        // proving the prefix width is accounted for during wrapping.
        let width = 14;
        let out = render("- alpha beta gamma delta epsilon zeta eta", width, &t);
        assert!(
            out.len() > 1,
            "expected the item to wrap onto multiple lines"
        );
        for line in &out {
            assert!(
                line_text(line).chars().count() <= width as usize,
                "line exceeded width {width}: {:?}",
                line_text(line)
            );
        }
    }

    #[test]
    fn render_code_block_is_indented_and_colored() {
        let t = Theme::wsx();
        let out = render("```\nfn main() {}\n```", 40, &t);
        assert_eq!(out.len(), 1);
        assert!(line_text(&out[0]).starts_with("  fn main() {}"));
        let colored = out[0].spans.iter().any(|s| s.style.fg == Some(t.code));
        assert!(colored);
    }

    #[test]
    fn render_blockquote_shows_bar_through_render() {
        let t = Theme::wsx();
        let out = render("> heads up", 40, &t);
        // Every quote line is prefixed with the "│ " bar in path color.
        assert!(out.iter().all(|l| line_text(l).starts_with("│ ")));
        let barred = out[0].spans.iter().any(|s| s.style.fg == Some(t.path));
        assert!(barred, "blockquote bar should use the path color");
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
}
