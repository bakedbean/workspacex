//! Markdown renderer for assistant text in the RECENT CHAT detail
//! module. Parses with `pulldown-cmark`, then wraps to the column width
//! ourselves, applying per-span styling for headings, lists, code,
//! blockquotes, and inline emphasis. Pure: `&str + width + &Theme` in,
//! `Vec<Line<'static>>` out.

use ratatui::style::Style;
use ratatui::text::Span;

#[derive(Clone, Debug)]
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
}
